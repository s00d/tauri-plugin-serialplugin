import "./styles.css";
import { mount } from 'svelte';
import App from "./App.svelte";

const app = mount(App, {
  target: document.getElementById("app")  as Element,
});

export default app;
