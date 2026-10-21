// SPDX-License-Identifier: MPL-2.0
// The Greetings tab: lists greetings the plugin holds and clears them on request.
//
// Text from the plugin is shown with textContent only: the view draws data it
// does not trust, and must never turn it into markup.

import { createBridge } from "./bridge.js";

const list = document.getElementById("greetings");
const empty = document.getElementById("empty");
const status = document.getElementById("status");
const clear = document.getElementById("clear");

// One list item: the greeting and the time it was made.
function item(greeting) {
  const row = document.createElement("li");
  const text = document.createElement("span");
  text.textContent = greeting.text;
  const time = document.createElement("time");
  const when = new Date(greeting.at);
  time.dateTime = when.toISOString();
  time.textContent = when.toLocaleTimeString();
  row.append(text, time);
  return row;
}

// Draw the list newest first, and the empty message when there is none.
function render(greetings) {
  list.replaceChildren(...greetings.slice().reverse().map(item));
  empty.hidden = greetings.length > 0;
  clear.disabled = greetings.length === 0;
}

function showError(error) {
  status.hidden = false;
  status.textContent = `Something went wrong: ${error.message}`;
}

// Connect, load the list, then keep it current from the plugin's events.
async function start() {
  let bridge;
  try {
    bridge = createBridge();
  } catch (error) {
    status.textContent = error.message;
    return;
  }
  let greetings = [];
  bridge.on("greeting-added", (greeting) => {
    greetings = [...greetings, greeting];
    render(greetings);
  });
  clear.addEventListener("click", async () => {
    try {
      await bridge.request("clear-greetings");
      greetings = [];
      render(greetings);
    } catch (error) {
      showError(error);
    }
  });
  try {
    const reply = await bridge.request("list-greetings");
    greetings = reply.greetings;
    status.hidden = true;
    render(greetings);
  } catch (error) {
    showError(error);
  }
}

start();
