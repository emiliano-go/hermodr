import { mount } from "svelte";
import Fixture from "./Fixture.svelte";
import Plugins from "./Plugins.svelte";
import GroupHistory from "./GroupHistory.svelte";

mount(location.search === "?group-history" ? GroupHistory : location.search === "?plugins" ? Plugins : Fixture, { target: document.getElementById("app")! });
