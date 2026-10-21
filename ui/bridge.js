// SPDX-License-Identifier: MPL-2.0
// The view's half of the Syrinx bridge: request ids, promises, timeouts and events.
//
// This file is the plugin's own; Syrinx injects nothing into the view. It
// speaks the envelope from research note 10, section 4:
//   { v: 1, kind: "request", id, method, body }   view -> plugin
//   { v: 1, kind: "response", id, ok, body | error } plugin -> view
//   { v: 1, kind: "event", topic, body }           plugin -> view
//   { v: 1, kind: "cancel", id }                   view -> plugin

export const BRIDGE_VERSION = 1;

/** A request the plugin refused, or one that never got an answer. */
export class BridgeError extends Error {
  constructor(kind, message) {
    super(message);
    this.name = "BridgeError";
    this.kind = kind;
  }
}

/**
 * Connect to the plugin through WebView2's built-in `chrome.webview` channel.
 * Pass a fake `webview` in tests. Returns `{ request, on }`.
 */
export function createBridge(webview = globalThis.chrome?.webview, { timeoutMs = 10_000 } = {}) {
  if (!webview) {
    throw new BridgeError("unavailable", "This page only works inside a Syrinx plugin view.");
  }
  let nextId = 1;
  const pending = new Map();
  const listeners = new Map();

  // Settle a waiting request; a response for an unknown id is dropped.
  function settle(message) {
    const waiting = pending.get(message.id);
    if (!waiting) return;
    pending.delete(message.id);
    clearTimeout(waiting.timer);
    if (message.ok === true) {
      waiting.resolve(message.body);
      return;
    }
    const error = message.error ?? {};
    waiting.reject(new BridgeError(error.kind ?? "internal", error.message ?? "The plugin failed."));
  }

  // Hand an event to everyone listening for its topic.
  function dispatch(message) {
    for (const listener of listeners.get(message.topic) ?? []) {
      listener(message.body);
    }
  }

  // Every message from the host; anything not in the envelope is ignored.
  webview.addEventListener("message", (event) => {
    const message = event.data;
    if (!message || message.v !== BRIDGE_VERSION) return;
    if (message.kind === "response") settle(message);
    if (message.kind === "event") dispatch(message);
  });

  /** Ask the plugin something. Only methods the manifest lists reach it. */
  function request(method, body = {}) {
    const id = nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        pending.delete(id);
        webview.postMessage({ v: BRIDGE_VERSION, kind: "cancel", id });
        reject(new BridgeError("timeout", `No answer to ${method} within ${timeoutMs} ms.`));
      }, timeoutMs);
      pending.set(id, { resolve, reject, timer });
      webview.postMessage({ v: BRIDGE_VERSION, kind: "request", id, method, body });
    });
  }

  /** Listen for an event topic; returns a function that stops listening. */
  function on(topic, listener) {
    const set = listeners.get(topic) ?? new Set();
    set.add(listener);
    listeners.set(topic, set);
    return () => set.delete(listener);
  }

  return { request, on };
}
