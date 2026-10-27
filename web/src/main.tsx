import { render } from "solid-js/web";
import { z } from "zod";
import App from "./App";
import "./styles.css";
z.config({ jitless: true });
render(() => <App />, document.getElementById("root")!);
