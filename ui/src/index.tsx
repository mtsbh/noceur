/* @refresh reload */
import { render } from "solid-js/web";
import App from "./App";
import "./theme.css";

/**
 * The web view supplies a browser context menu — Back, Refresh, Save as, Print —
 * which is meaningless in a file browser and actively misleading: none of those
 * entries do anything a user of this app wants. Suppressing it lets the list put
 * its own menu there instead.
 */
document.addEventListener("contextmenu", (e) => e.preventDefault());

const root = document.getElementById("root");
if (!root) throw new Error("#root not found");
render(() => <App />, root);
