import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import { startTheme } from "./lib/theme.svelte";

// 挂载前先应用色调，第一帧就是正确的颜色
startTheme();

export default mount(App, { target: document.getElementById("app")! });
