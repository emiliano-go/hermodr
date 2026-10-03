import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { parse } from "svelte/compiler";

test("message rail keeps zero overscan and omits SPA SSR mount hints", () => {
  const ast = parse(readFileSync(new URL("../lib/messages/MessageList.svelte", import.meta.url), "utf8"), { modern: true });
  const wrapper = ast.fragment.nodes.find((node) => node.type === "RegularElement" && node.name === "div");
  assert.ok(wrapper && wrapper.type === "RegularElement");
  const list = wrapper.fragment.nodes.find((node) => node.type === "Component" && node.name === "VList");
  assert.ok(list && list.type === "Component");
  const buffer = list.attributes.find((attribute) => attribute.type === "Attribute" && attribute.name === "bufferSize");
  assert.ok(buffer && buffer.type === "Attribute" && buffer.value !== true);
  const value = Array.isArray(buffer.value) ? buffer.value[0] : buffer.value;
  assert.equal(value.type, "ExpressionTag");
  assert.ok(value.type === "ExpressionTag" && value.expression.type === "Literal");
  assert.equal(value.expression.value, 0);
  assert.equal(list.attributes.some((attribute) => attribute.type === "Attribute" && attribute.name === "ssrCount"), false);
});
