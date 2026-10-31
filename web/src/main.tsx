import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./app.css";
import { applyTheme, getTheme } from "./theme";

applyTheme(getTheme());
const root = document.getElementById("root");
if (root) createRoot(root).render(<StrictMode><App /></StrictMode>);
