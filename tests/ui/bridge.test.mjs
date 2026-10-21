// SPDX-License-Identifier: MPL-2.0
// Tests ui/bridge.js against a fake chrome.webview: `node --test tests/ui`.

import assert from "node:assert/strict";
import { test } from "node:test";

import { BridgeError, createBridge } from "../../ui/bridge.js";

// A stand-in for WebView2's chrome.webview: records posts, delivers replies.
function fakeWebview() {
  const posted = [];
  let listener = null;
  return {
    posted,
    postMessage: (message) => posted.push(message),
    addEventListener: (type, handler) => {
      assert.equal(type, "message");
      listener = handler;
    },
    deliver: (data) => listener({ data }),
  };
}

test("a request is posted in the envelope and resolves with the body", async () => {
  const webview = fakeWebview();
  const bridge = createBridge(webview);
  const reply = bridge.request("list-greetings");
  assert.deepEqual(webview.posted[0], { v: 1, kind: "request", id: 1, method: "list-greetings", body: {} });
  webview.deliver({ v: 1, kind: "response", id: 1, ok: true, body: { greetings: [] } });
  assert.deepEqual(await reply, { greetings: [] });
});

test("each request gets its own id, and replies may arrive out of order", async () => {
  const webview = fakeWebview();
  const bridge = createBridge(webview);
  const first = bridge.request("list-greetings");
  const second = bridge.request("clear-greetings");
  assert.deepEqual(webview.posted.map((m) => m.id), [1, 2]);
  webview.deliver({ v: 1, kind: "response", id: 2, ok: true, body: { cleared: 0 } });
  webview.deliver({ v: 1, kind: "response", id: 1, ok: true, body: { greetings: [] } });
  assert.deepEqual(await second, { cleared: 0 });
  assert.deepEqual(await first, { greetings: [] });
});

test("a refusal rejects with the plugin's error kind", async () => {
  const webview = fakeWebview();
  const reply = createBridge(webview).request("list-greetings");
  webview.deliver({ v: 1, kind: "response", id: 1, ok: false, error: { kind: "denied", message: "storage" } });
  await assert.rejects(reply, (error) => error instanceof BridgeError && error.kind === "denied");
});

test("no answer in time rejects and sends a cancel", async () => {
  const webview = fakeWebview();
  const reply = createBridge(webview, { timeoutMs: 5 }).request("list-greetings");
  await assert.rejects(reply, (error) => error.kind === "timeout");
  assert.deepEqual(webview.posted[1], { v: 1, kind: "cancel", id: 1 });
});

test("events reach their topic's listeners until they unsubscribe", () => {
  const webview = fakeWebview();
  const bridge = createBridge(webview);
  const heard = [];
  const stop = bridge.on("greeting-added", (body) => heard.push(body.text));
  webview.deliver({ v: 1, kind: "event", topic: "greeting-added", body: { text: "Hi, Ada!" } });
  webview.deliver({ v: 1, kind: "event", topic: "other", body: { text: "ignored" } });
  stop();
  webview.deliver({ v: 1, kind: "event", topic: "greeting-added", body: { text: "late" } });
  assert.deepEqual(heard, ["Hi, Ada!"]);
});

test("messages outside the envelope and stale responses are ignored", async () => {
  const webview = fakeWebview();
  const reply = createBridge(webview).request("list-greetings");
  webview.deliver({ v: 2, kind: "response", id: 1, ok: true, body: "wrong version" });
  webview.deliver("not an envelope");
  webview.deliver({ v: 1, kind: "response", id: 99, ok: true, body: "unknown id" });
  webview.deliver({ v: 1, kind: "response", id: 1, ok: true, body: "right" });
  assert.equal(await reply, "right");
});

test("outside Syrinx there is no channel, and the bridge says so", () => {
  assert.throws(() => createBridge(undefined), (error) => error.kind === "unavailable");
});
