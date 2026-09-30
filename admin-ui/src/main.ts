import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";

const target = document.getElementById("app");
if (!target) {
  throw new Error("missing #app mount point");
}

// Svelte 5 components are functions; `new App(...)` throws at startup.
const app = mount(App, { target });
export default app;
