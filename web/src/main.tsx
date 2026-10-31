import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { relayIfNeeded } from "./auth";
import "./app.css";
import { applyTheme, getTheme } from "./theme";

applyTheme(getTheme());
const root = document.getElementById("root");
// `/relay` only hands a sign-in code on to a workspace site; it shows nothing.
if (!relayIfNeeded() && root) createRoot(root).render(<StrictMode><App /></StrictMode>);
