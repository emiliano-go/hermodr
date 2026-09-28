import { mount } from "svelte";
import Fixture from "./Fixture.svelte";
import Plugins from "./Plugins.svelte";

mount(location.search === "?plugins" ? Plugins : Fixture, { target: document.getElementById("app")! });
