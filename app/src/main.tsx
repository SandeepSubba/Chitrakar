import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import "./styles.css";
import { keepForOffline } from "./offline";
import { watchWindowFocus } from "./windowFocus";
import { watchForEngineTraps } from "./engineGuard";

keepForOffline();
watchWindowFocus();
watchForEngineTraps();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
