//! The BI connection: Arrow Flight SQL, so a BI tool, a notebook or any ADBC or JDBC client can run SQL and list tables with the
//! credential it already has.
//!
//! * Sign-in is the handshake: user name = the workspace's id, password = an access token (a person's or a service principal's). The
//!   answer is a bearer token the client sends back, `<workspace>~<token>`; a client may also send that bearer token without a handshake.
//!   The token is checked on every call by the same code the HTTP API uses ([`identity_of`]), so it means the same thing everywhere.
//! * A statement is an ordinary run: authorized as the caller, rewritten with row filters and column masks, audited, and run on the
//!   workspace's compute with the whole result kept as an Arrow file. The reader is then sent that file's batches. Nothing here decides
//!   who may see what; it only asks the services that do.
//! * Tables are listed with the same visibility rule as the catalog screens: what a caller may not see is not there.

use std::{net::SocketAddr, pin::Pin, time::Duration};

use arrow_flight::{
    FlightDescriptor, FlightEndpoint, FlightInfo, HandshakeRequest, HandshakeResponse, Ticket,
    encode::FlightDataEncoderBuilder,
    error::FlightError,
    flight_service_server::FlightServiceServer,
    sql::{
        CommandGetCatalogs, CommandGetDbSchemas, CommandGetTables, CommandStatementQuery,
        ProstMessageExt, SqlInfo, TicketStatementQuery,
        metadata::{GetCatalogsBuilder, GetDbSchemasBuilder, GetTablesBuilder},
        server::FlightSqlService,
    },
};
use arrow_ipc::reader::StreamReader;
use futures_util::{Stream, StreamExt, TryStreamExt};
use prost::Message;
use tonic::{Request, Response, Status, Streaming, metadata::MetadataMap, transport::Server};

use crate::{
    compute::service::{self as compute, Ctx, SqlRequest},
    governance::{
        catalogs::{model::Kind, service as catalogs},
        context::{WsCtx, identity_of},
    },
    kernel::{error::Error, state::Shared},
};

/// How long a statement may run before the client is told it is still running (a BI tool then asks again).
const STATEMENT_WAIT: Duration = Duration::from_mins(2);
/// How much of a result file is read to learn its columns.
const SCHEMA_PREFIX: usize = 256 * 1024;

type Flights =
    Pin<Box<dyn Stream<Item = Result<arrow_flight::FlightData, Status>> + Send + 'static>>;

#[derive(Clone)]
pub struct Service {
    app: Shared,
}

/// What a person sees of an error: the same words as the HTTP API, never an internal detail.
fn status(e: Error) -> Status {
    match e {
        Error::Unauthorized => {
            Status::unauthenticated("sign in with a workspace id and an access token")
        }
        Error::NotFound(m) => Status::not_found(m),
        Error::Invalid(m) | Error::Unsupported(m) => Status::invalid_argument(m),
        Error::Denied(_) => Status::permission_denied("you do not have permission to do that"),
        Error::Conflict(m) => Status::aborted(m),
        Error::TooManyRequests(m) => Status::resource_exhausted(m),
        Error::Unavailable(m) => Status::unavailable(m),
        other => {
            tracing::warn!(error = ?other, "a BI request failed");
            Status::internal("something went wrong on our side")
        }
    }
}

/// `<workspace>~<token>` from a bearer header, or a workspace and token from a Basic header.
fn credentials(md: &MetadataMap) -> Result<(String, String), Status> {
    let header = md
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Status::unauthenticated("sign in first"))?;
    if let Some(bearer) = header.strip_prefix("Bearer ") {
        let (workspace, token) = bearer
            .split_once('~')
            .ok_or_else(|| Status::unauthenticated("sign in first"))?;
        return Ok((workspace.to_owned(), token.to_owned()));
    }
    if let Some(basic) = header.strip_prefix("Basic ") {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let decoded = STANDARD
            .decode(basic)
            .map_err(|_| Status::unauthenticated("the credentials are not readable"))?;
        let text = String::from_utf8(decoded)
            .map_err(|_| Status::unauthenticated("the credentials are not readable"))?;
        let (workspace, token) = text.split_once(':').ok_or_else(|| {
            Status::unauthenticated(
                "use the workspace id as the user name and an access token as the password",
            )
        })?;
        return Ok((workspace.to_owned(), token.to_owned()));
    }
    Err(Status::unauthenticated("sign in first"))
}

impl Service {
    /// Who is calling and in which workspace: worked out again on every call.
    async fn context(&self, md: &MetadataMap) -> Result<WsCtx, Status> {
        let (workspace, token) = credentials(md)?;
        let identity = identity_of(&self.app, &token).await.map_err(status)?;
        WsCtx::open(&self.app, identity, &workspace)
            .await
            .map_err(status)
    }
}

fn ctx_of(c: &WsCtx) -> Ctx<'_> {
    Ctx {
        identity: &c.identity,
        workspace: &c.workspace,
        standing: c.standing,
        sides: &c.sides,
        schedule_task_id: None,
    }
}

/// The columns of an Arrow IPC stream, from its first bytes.
async fn schema_of(
    mut file: loamz_lake::objects::Reading,
) -> Result<arrow_schema::SchemaRef, Status> {
    let mut head = Vec::new();
    while head.len() < SCHEMA_PREFIX {
        match file.next().await {
            Some(Ok(chunk)) => head.extend_from_slice(&chunk),
            Some(Err(_)) => return Err(Status::internal("the result could not be read")),
            None => break,
        }
    }
    let reader = StreamReader::try_new(std::io::Cursor::new(head), None)
        .map_err(|_| Status::internal("the result could not be read"))?;
    Ok(reader.schema())
}

/// A `FlightInfo` that points back at this server with `ticket`.
fn info(
    schema: &arrow_schema::Schema,
    ticket: Ticket,
    descriptor: FlightDescriptor,
    rows: i64,
    bytes: i64,
) -> Result<FlightInfo, Status> {
    Ok(FlightInfo::new()
        .try_with_schema(schema)
        .map_err(|_| Status::internal("the columns could not be described"))?
        .with_endpoint(FlightEndpoint::new().with_ticket(ticket))
        .with_descriptor(descriptor)
        .with_total_records(rows)
        .with_total_bytes(bytes))
}

/// The batches of a metadata answer as a Flight stream.
fn batch_stream(schema: arrow_schema::SchemaRef, batch: arrow_array::RecordBatch) -> Flights {
    let batches = futures_util::stream::iter(vec![Ok(batch)]);
    FlightDataEncoderBuilder::new()
        .with_schema(schema)
        .build(batches)
        .map_err(|e| Status::internal(e.to_string()))
        .boxed()
}

#[tonic::async_trait]
impl FlightSqlService for Service {
    type FlightService = Service;

    async fn do_handshake(
        &self,
        request: Request<Streaming<HandshakeRequest>>,
    ) -> Result<
        Response<Pin<Box<dyn Stream<Item = Result<HandshakeResponse, Status>> + Send>>>,
        Status,
    > {
        let (workspace, token) = credentials(request.metadata())?;
        // Prove the credentials now, so a wrong one fails at sign-in and not at the first statement.
        let identity = identity_of(&self.app, &token).await.map_err(status)?;
        WsCtx::open(&self.app, identity, &workspace)
            .await
            .map_err(status)?;
        let bearer = format!("{workspace}~{token}");
        let reply = HandshakeResponse {
            protocol_version: 0,
            payload: bearer.clone().into(),
        };
        let mut response = Response::new(Box::pin(futures_util::stream::iter(vec![Ok(reply)]))
            as Pin<Box<dyn Stream<Item = Result<HandshakeResponse, Status>> + Send>>);
        if let Ok(value) = format!("Bearer {bearer}").parse() {
            response.metadata_mut().insert("authorization", value);
        }
        Ok(response)
    }

    async fn get_flight_info_statement(
        &self,
        query: CommandStatementQuery,
        request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        let c = self.context(request.metadata()).await?;
        let k = ctx_of(&c);
        let run = compute::submit_sql(
            &self.app,
            &k,
            SqlRequest {
                sql: query.query,
                limit: None,
                target: None,
                export: true,
                save: None,
                idempotency_key: None,
            },
        )
        .await
        .map_err(status)?;
        let run = compute::wait(&self.app, &k, run, STATEMENT_WAIT)
            .await
            .map_err(status)?;
        match run.state.as_str() {
            "succeeded" => {}
            "failed" | "cancelled" | "timed_out" => {
                return Err(Status::invalid_argument(if run.error.is_empty() {
                    format!("the statement {}", run.state)
                } else {
                    run.error
                }));
            }
            _ => {
                return Err(Status::deadline_exceeded(
                    "the statement is still running: ask again",
                ));
            }
        }
        let (file, bytes) = compute::open_result(&self.app, &k, &run.id)
            .await
            .map_err(status)?;
        let schema = schema_of(file).await?;
        let rows = serde_json::from_str::<serde_json::Value>(&run.result)
            .ok()
            .and_then(|v| v["export"]["rows"].as_i64())
            .unwrap_or(-1);
        let ticket = Ticket {
            ticket: TicketStatementQuery {
                statement_handle: run.id.into_bytes().into(),
            }
            .as_any()
            .encode_to_vec()
            .into(),
        };
        Ok(Response::new(info(
            &schema,
            ticket,
            request.into_inner(),
            rows,
            i64::try_from(bytes).unwrap_or(-1),
        )?))
    }

    async fn do_get_statement(
        &self,
        ticket: TicketStatementQuery,
        request: Request<Ticket>,
    ) -> Result<
        Response<<Self as arrow_flight::flight_service_server::FlightService>::DoGetStream>,
        Status,
    > {
        let c = self.context(request.metadata()).await?;
        let id = String::from_utf8(ticket.statement_handle.to_vec())
            .map_err(|_| Status::invalid_argument("not a statement of this server"))?;
        let (file, _) = compute::open_result(&self.app, &ctx_of(&c), &id)
            .await
            .map_err(status)?;
        // The file is an Arrow IPC stream: read its batches on a thread of their own and send them on as they come.
        let reader = tokio_util::io::StreamReader::new(file);
        let (sender, mut receiver) = tokio::sync::mpsc::channel(4);
        let handle = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            let _guard = handle.enter();
            let bridge = tokio_util::io::SyncIoBridge::new(reader);
            let batches = match StreamReader::try_new(bridge, None) {
                Ok(b) => b,
                Err(e) => {
                    let _ = sender.blocking_send(Err(FlightError::from(e)));
                    return;
                }
            };
            for batch in batches {
                if sender
                    .blocking_send(batch.map_err(FlightError::from))
                    .is_err()
                {
                    return;
                }
            }
        });
        let batches = futures_util::stream::poll_fn(move |cx| receiver.poll_recv(cx));
        let stream = FlightDataEncoderBuilder::new()
            .build(batches)
            .map_err(|e| Status::internal(e.to_string()))
            .boxed();
        Ok(Response::new(stream))
    }

    async fn get_flight_info_catalogs(
        &self,
        query: CommandGetCatalogs,
        request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        self.context(request.metadata()).await?;
        let ticket = Ticket {
            ticket: query.as_any().encode_to_vec().into(),
        };
        Ok(Response::new(info(
            &query.into_builder().schema(),
            ticket,
            request.into_inner(),
            -1,
            -1,
        )?))
    }

    async fn do_get_catalogs(
        &self,
        query: CommandGetCatalogs,
        request: Request<Ticket>,
    ) -> Result<
        Response<<Self as arrow_flight::flight_service_server::FlightService>::DoGetStream>,
        Status,
    > {
        let c = self.context(request.metadata()).await?;
        let mut builder: GetCatalogsBuilder = query.into_builder();
        for entry in self.visible(&c, "").await? {
            if entry.kind == Kind::Catalog {
                builder.append(entry.name);
            }
        }
        let schema = builder.schema();
        let batch = builder
            .build()
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(batch_stream(schema, batch)))
    }

    async fn get_flight_info_schemas(
        &self,
        query: CommandGetDbSchemas,
        request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        self.context(request.metadata()).await?;
        let ticket = Ticket {
            ticket: query.as_any().encode_to_vec().into(),
        };
        Ok(Response::new(info(
            &query.into_builder().schema(),
            ticket,
            request.into_inner(),
            -1,
            -1,
        )?))
    }

    async fn do_get_schemas(
        &self,
        query: CommandGetDbSchemas,
        request: Request<Ticket>,
    ) -> Result<
        Response<<Self as arrow_flight::flight_service_server::FlightService>::DoGetStream>,
        Status,
    > {
        let c = self.context(request.metadata()).await?;
        let mut builder: GetDbSchemasBuilder = query.into_builder();
        for catalog in self.visible(&c, "").await? {
            if catalog.kind != Kind::Catalog {
                continue;
            }
            for schema in self.visible(&c, &catalog.id).await? {
                if schema.kind == Kind::Schema {
                    builder.append(&catalog.name, schema.name);
                }
            }
        }
        let schema = builder.schema();
        let batch = builder
            .build()
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(batch_stream(schema, batch)))
    }

    async fn get_flight_info_tables(
        &self,
        query: CommandGetTables,
        request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        self.context(request.metadata()).await?;
        let ticket = Ticket {
            ticket: query.as_any().encode_to_vec().into(),
        };
        Ok(Response::new(info(
            &query.into_builder().schema(),
            ticket,
            request.into_inner(),
            -1,
            -1,
        )?))
    }

    async fn do_get_tables(
        &self,
        query: CommandGetTables,
        request: Request<Ticket>,
    ) -> Result<
        Response<<Self as arrow_flight::flight_service_server::FlightService>::DoGetStream>,
        Status,
    > {
        let c = self.context(request.metadata()).await?;
        let mut builder: GetTablesBuilder = query.into_builder();
        let empty = arrow_schema::Schema::empty();
        for catalog in self.visible(&c, "").await? {
            if catalog.kind != Kind::Catalog {
                continue;
            }
            for schema in self.visible(&c, &catalog.id).await? {
                if schema.kind != Kind::Schema {
                    continue;
                }
                for table in self.visible(&c, &schema.id).await? {
                    let kind = match table.kind {
                        Kind::Table => "TABLE",
                        Kind::View => "VIEW",
                        _ => continue,
                    };
                    builder
                        .append(&catalog.name, &schema.name, table.name, kind, &empty)
                        .map_err(|e| Status::internal(e.to_string()))?;
                }
            }
        }
        let schema = builder.schema();
        let batch = builder
            .build()
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(batch_stream(schema, batch)))
    }

    async fn register_sql_info(&self, _id: i32, _result: &SqlInfo) {}
}

impl Service {
    /// The children of a securable that the caller may see (the same rule as the catalog screens).
    async fn visible(
        &self,
        c: &WsCtx,
        parent: &str,
    ) -> Result<Vec<crate::governance::catalogs::model::Securable>, Status> {
        catalogs::list(
            &self.app,
            &c.identity,
            &c.workspace.id,
            &c.workspace.org_id,
            &c.sides,
            parent,
        )
        .await
        .map_err(status)
    }
}

/// Serves the BI connection on `addr` until the process stops.
///
/// # Errors
/// The address could not be bound.
pub async fn serve(app: Shared, addr: SocketAddr) -> Result<(), String> {
    tracing::info!(%addr, "the BI connection (Arrow Flight SQL) is listening");
    Server::builder()
        .add_service(FlightServiceServer::new(Service { app }))
        .serve(addr)
        .await
        .map_err(|e| e.to_string())
}
