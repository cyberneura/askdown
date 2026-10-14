"use strict";

const { invoke } = window.__TAURI__.core;

const form = document.getElementById("questions");
const statusLine = document.getElementById("status");

// One entry per question: how to read its answer back out of the form.
const collectors = [];

function element(tag, attributes = {}, children = []) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attributes)) {
    if (key === "text") node.textContent = value;
    else if (value === true) node.setAttribute(key, "");
    else if (value !== false && value != null) node.setAttribute(key, value);
  }
  for (const child of children) node.append(child);
  return node;
}

// The HTML has been sanitised in Rust (askdown_core::markdown), so it is safe to insert.
function trustedHtml(html) {
  const holder = element("div", { class: "prose" });
  holder.innerHTML = html;
  return holder;
}

function renderChoice(segment, body) {
  const { question, id, previous } = segment;
  const type = question.multiple ? "checkbox" : "radio";
  const selected = new Set(previous?.selected ?? []);
  const list = element("div", { class: "options" });
  question.options.forEach((option, index) => {
    const value = typeof option === "string" ? option : option.value ?? option.label ?? option.src ?? "";
    const label = typeof option === "string" ? option : option.label ?? value;
    const input = element("input", {
      type,
      name: id,
      value,
      id: `${id}-${index}`,
      checked: selected.has(value),
    });
    list.append(element("label", { class: "option", for: `${id}-${index}` }, [input, element("span", { text: label })]));
  });
  body.append(list);

  let other = null;
  if (question.other) {
    other = element("input", {
      type: "text",
      class: "other",
      placeholder: "Other…",
      value: previous?.other ?? "",
    });
    body.append(other);
  }

  return () => {
    const values = [...form.querySelectorAll(`input[name="${CSS.escape(id)}"]:checked`)].map((input) => input.value);
    const answer = { selected: values };
    if (other && other.value.trim() !== "") answer.other = other.value;
    return values.length || answer.other ? answer : null;
  };
}

function renderText(segment, body) {
  const { question, previous } = segment;
  const attributes = {
    placeholder: question.placeholder ?? "",
    class: "text",
  };
  const field = question.multiline
    ? element("textarea", { ...attributes, rows: 5 })
    : element("input", { ...attributes, type: "text" });
  field.value = previous ?? question.default ?? "";
  body.append(field);
  return () => (field.value.trim() === "" ? null : field.value);
}

const renderers = {
  choice: renderChoice,
  text: renderText,
};

function renderQuestion(segment) {
  const card = element("section", { class: "question", "data-id": segment.id });
  card.append(element("div", { class: "number", text: `Q${segment.number} · ${segment.id}` }));

  if (segment.error) {
    card.classList.add("invalid");
    card.append(element("pre", { class: "error", text: segment.error }));
    return card;
  }
  if (segment.html) card.append(trustedHtml(segment.html));

  const body = element("div", { class: "body" });
  card.append(body);
  const render = renderers[segment.question.type];
  if (!render) {
    body.append(element("p", { class: "unsupported", text: `"${segment.question.type}" questions are not supported yet.` }));
    return card;
  }
  const collect = render(segment, body);
  collectors.push({ id: segment.id, required: segment.question.required, card, collect });
  return card;
}

async function load() {
  const view = await invoke("document");
  document.getElementById("path").textContent = view.path;
  form.replaceChildren(
    ...view.segments.map((segment) => (segment.kind === "prose" ? trustedHtml(segment.html) : renderQuestion(segment))),
  );
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  const answers = {};
  const missing = [];
  for (const { id, required, card, collect } of collectors) {
    const value = collect();
    card.classList.toggle("missing", required && value == null);
    if (value != null) answers[id] = value;
    else if (required) missing.push(id);
  }
  if (missing.length) {
    statusLine.textContent = `Required: ${missing.join(", ")}`;
    form.querySelector(".missing")?.scrollIntoView({ behavior: "smooth", block: "center" });
    return;
  }
  try {
    await invoke("submit", { answers });
  } catch (error) {
    statusLine.textContent = String(error);
  }
});

document.getElementById("cancel").addEventListener("click", () => invoke("cancel"));

// Links go nowhere inside the app window; opening them in the browser is still to be done.
document.addEventListener("click", (event) => {
  if (event.target.closest("a[href]")) event.preventDefault();
});

load().catch((error) => {
  statusLine.textContent = String(error);
});
