import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import "./styles.css";
import { keepForOffline } from "./offline";
import { keepPanelFocus, watchWindowFocus } from "./windowFocus";
import { watchForEngineTraps } from "./engineGuard";
import { holdKeysWhileComposing } from "./composing";

// First, so that no other listener hears a key an input method is using.
holdKeysWhileComposing();
keepForOffline();
watchWindowFocus();
keepPanelFocus();
watchForEngineTraps();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
