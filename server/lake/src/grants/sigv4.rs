//! AWS Signature Version 4, for the one kind of request the server signs itself: an STS call with a form body. (Reads and writes
//! of objects are signed by the object-store library.)

use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub struct Signer<'a> {
    pub key_id: &'a str,
    pub secret: &'a str,
    pub region: &'a str,
    pub service: &'a str,
}

impl Signer<'_> {
    /// The `Authorization` header value. `headers` are lower-case names with their values, sorted by name; every one is signed.
    #[must_use]
    pub fn authorization(
        &self,
        method: &str,
        path: &str,
        query: &str,
        headers: &[(&str, &str)],
        payload: &[u8],
        amz_date: &str,
    ) -> String {
        let day = &amz_date[..8];
        let names = headers
            .iter()
            .map(|(n, _)| *n)
            .collect::<Vec<_>>()
            .join(";");
        let canonical_headers: String = headers
            .iter()
            .map(|(n, v)| [n, ":", v, "\n"].concat())
            .collect();
        let canonical = format!(
            "{method}\n{path}\n{query}\n{canonical_headers}\n{names}\n{}",
            hex::encode(Sha256::digest(payload))
        );
        let scope = format!("{day}/{}/{}/aws4_request", self.region, self.service);
        let to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
            hex::encode(Sha256::digest(canonical.as_bytes()))
        );
        let mut key = hmac(format!("AWS4{}", self.secret).as_bytes(), day.as_bytes());
        for part in [self.region, self.service, "aws4_request"] {
            key = hmac(&key, part.as_bytes());
        }
        let signature = hex::encode(hmac(&key, to_sign.as_bytes()));
        format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={names}, Signature={signature}",
            self.key_id
        )
    }
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    // HMAC accepts a key of any length.
    let mut mac = HmacSha256::new_from_slice(key).unwrap_or_else(|_| unreachable!());
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// `YYYYMMDDTHHMMSSZ`, the form `x-amz-date` takes.
#[must_use]
pub fn amz_date(now: SystemTime) -> String {
    let secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil(i64::try_from(days).unwrap_or(0));
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Year, month and day of a day count since 1970-01-01 (Howard Hinnant's algorithm).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// The worked example in AWS's Signature Version 4 documentation (IAM `ListUsers`).
    #[test]
    fn matches_the_aws_documentation_example() {
        let signer = Signer {
            key_id: "AKIDEXAMPLE",
            secret: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            region: "us-east-1",
            service: "iam",
        };
        let auth = signer.authorization(
            "GET",
            "/",
            "Action=ListUsers&Version=2010-05-08",
            &[
                (
                    "content-type",
                    "application/x-www-form-urlencoded; charset=utf-8",
                ),
                ("host", "iam.amazonaws.com"),
                ("x-amz-date", "20150830T123600Z"),
            ],
            b"",
            "20150830T123600Z",
        );
        assert!(
            auth.ends_with(
                "Signature=5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"
            ),
            "{auth}"
        );
    }

    #[test]
    fn dates_are_formatted_from_the_epoch() {
        assert_eq!(amz_date(UNIX_EPOCH), "19700101T000000Z");
        let t = UNIX_EPOCH + Duration::from_hours(400_260) + Duration::from_mins(36);
        assert_eq!(amz_date(t), "20150830T123600Z");
        let leap = UNIX_EPOCH + Duration::from_hours(474_768); // 2024-02-29
        assert_eq!(amz_date(leap), "20240229T000000Z");
    }
}
