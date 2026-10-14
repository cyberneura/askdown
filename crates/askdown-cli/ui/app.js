"use strict";

const { invoke } = window.__TAURI__.core;

const form = document.getElementById("questions");
const statusLine = document.getElementById("status");
const submitButton = document.getElementById("submit");

// Prefix that turns a path relative to the Markdown file into a URL (set by `load`).
let mediaBase = "";

// One entry per question: how to read its answer back out of the form.
// `collect` returns the answer, null when unanswered, or throws with a message
// when what is there is not a valid answer. `finalize`, when present, is run
// after every question has passed `collect` and may replace the answer (the
// drawing is saved to disk at that point).
const collectors = [];

function element(tag, attributes = {}, children = []) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attributes)) {
    if (key === "text") node.textContent = value;
    else if (value === true) node.setAttribute(key, "");
    else if (value !== false && value != null) node.setAttribute(key, value);
  }
  for (const child of children) if (child != null) node.append(child);
  return node;
}

function button(text, onClick, attributes = {}) {
  const node = element("button", { type: "button", ...attributes, text });
  node.addEventListener("click", onClick);
  return node;
}

// The HTML has been sanitised in Rust (askdown_core::markdown), so it is safe to insert.
function trustedHtml(html) {
  const holder = element("div", { class: "prose" });
  holder.innerHTML = html;
  return holder;
}

// Media paths in the YAML are relative to the Markdown file, like those in the
// prose; the Rust side rewrites the prose, this does the same for questions.
// A path here is a file name, not a URL: `#` and `%` in it are literal.
function mediaUrl(src) {
  if (/^[a-z][a-z0-9+.-]*:/i.test(src) || src.startsWith("/")) return src;
  return mediaBase + src.replace(/^\.\//, "").split("/").map(encodeURIComponent).join("/");
}

function mediaKind(src) {
  const extension = (src.match(/\.([a-z0-9]+)$/i)?.[1] ?? "").toLowerCase();
  if (["mp4", "m4v", "webm", "mov", "ogv", "mkv"].includes(extension)) return "video";
  if (["mp3", "m4a", "wav", "ogg", "oga", "aac", "flac", "opus"].includes(extension)) return "audio";
  return "image";
}

function mediaElement(src, attributes = {}) {
  const kind = mediaKind(src);
  if (kind === "image") return element("img", { src: mediaUrl(src), alt: src });
  const media = element(kind, { src: mediaUrl(src), preload: "auto", ...attributes });
  if (kind === "video") showFirstFrame(media);
  return media;
}

// WebKit paints nothing until the video is played or seeked, which leaves the
// frame to click on (or to pick) blank. A seek by a hair makes it paint.
function showFirstFrame(video) {
  video.addEventListener("loadedmetadata", () => {
    if (video.currentTime === 0) video.currentTime = 0.001;
  });
}

function optionValue(option) {
  return typeof option === "string" ? option : (option.value ?? option.label ?? option.src ?? "");
}

function optionLabel(option) {
  return typeof option === "string" ? option : (option.label ?? optionValue(option));
}

// The `default` of a choice: the answer's shape (`{ selected: [...] }`), or for
// convenience a value or a list of values.
function selectedDefault(value) {
  if (value == null) return [];
  if (typeof value === "object" && !Array.isArray(value)) return (value.selected ?? []).map(String);
  return Array.isArray(value) ? value.map(String) : [String(value)];
}

function formatTime(seconds) {
  if (!Number.isFinite(seconds)) return "–";
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${(seconds - minutes * 60).toFixed(1).padStart(4, "0")}`;
}

const round = (value, places) => Number(value.toFixed(places));

function renderChoice(segment, body) {
  const { question, id, previous } = segment;
  const type = question.multiple ? "checkbox" : "radio";
  const selected = new Set(previous?.selected ?? selectedDefault(question.default));
  const list = element("div", { class: "options" });
  question.options.forEach((option, index) => {
    const value = optionValue(option);
    const input = element("input", {
      type,
      name: id,
      value,
      id: `${id}-${index}`,
      checked: selected.has(value),
    });
    const description = typeof option === "string" ? null : option.description;
    list.append(
      element("label", { class: "option", for: `${id}-${index}` }, [
        input,
        element("span", {}, [
          element("span", { text: optionLabel(option) }),
          description ? element("span", { class: "description", text: description }) : null,
        ]),
      ]),
    );
  });
  body.append(list);

  let other = null;
  if (question.other) {
    other = element("input", {
      type: "text",
      class: "other",
      placeholder: "Other…",
      value: previous?.other ?? question.default?.other ?? "",
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

function renderMediaChoice(segment, body) {
  const { question, id, previous } = segment;
  const type = question.multiple ? "checkbox" : "radio";
  const selected = new Set(previous?.selected ?? selectedDefault(question.default));
  const grid = element("div", { class: "media-options" });
  question.options.forEach((option, index) => {
    // A bare string is the file to show, and names the option too.
    const src = typeof option === "string" ? option : option.src;
    const value = optionValue(option);
    const input = element("input", {
      type,
      name: id,
      value,
      id: `${id}-${index}`,
      checked: selected.has(value),
    });
    const media = src ? mediaElement(src, { controls: true }) : null;
    const card = element("div", { class: "media-option" }, [
      media,
      element("label", { class: "caption", for: `${id}-${index}` }, [
        input,
        element("span", {}, [
          element("span", { text: optionLabel(option) }),
          typeof option !== "string" && option.description
            ? element("span", { class: "description", text: option.description })
            : null,
        ]),
      ]),
    ]);
    // Clicking a picture picks it; video and audio keep their controls, so
    // only their caption does.
    if (media?.tagName === "IMG") {
      media.addEventListener("click", () => {
        input.checked = type === "radio" ? true : !input.checked;
        input.dispatchEvent(new Event("change", { bubbles: true }));
      });
    }
    input.addEventListener("change", () => {
      for (const node of grid.querySelectorAll(".media-option")) {
        node.classList.toggle("selected", node.querySelector("input").checked);
      }
    });
    card.classList.toggle("selected", input.checked);
    grid.append(card);
  });
  body.append(grid);

  return () => {
    const values = [...form.querySelectorAll(`input[name="${CSS.escape(id)}"]:checked`)].map((input) => input.value);
    return values.length ? { selected: values } : null;
  };
}

// Play / pause, a seek bar and the time, for a video that must take clicks on
// its picture (the native controls would take them instead).
function videoControls(video) {
  const play = button("▶", () => (video.paused ? video.play() : video.pause()), { class: "play" });
  const seek = element("input", { type: "range", min: 0, max: 0, step: 0.01, value: 0, class: "seek" });
  const time = element("span", { class: "time", text: "0:00.0 / –" });
  let seeking = false;
  const update = () => {
    if (!seeking) seek.value = video.currentTime;
    time.textContent = `${formatTime(video.currentTime)} / ${formatTime(video.duration)}`;
    play.textContent = video.paused ? "▶" : "❚❚";
  };
  video.addEventListener("loadedmetadata", () => {
    seek.max = video.duration;
    update();
  });
  for (const event of ["timeupdate", "play", "pause", "seeked"]) video.addEventListener(event, update);
  seek.addEventListener("pointerdown", () => (seeking = true));
  for (const event of ["pointerup", "pointercancel", "change"]) seek.addEventListener(event, () => (seeking = false));
  seek.addEventListener("input", () => {
    video.currentTime = Number(seek.value);
    update();
  });
  return element("div", { class: "controls" }, [play, seek, time]);
}

function renderPoint(segment, body) {
  const { question, id, previous } = segment;
  if (!question.src) return unusable(body, "a point question needs `src`");
  const kind = mediaKind(question.src);
  if (kind === "audio") return unusable(body, "a point question needs an image or a video");
  const media = mediaElement(question.src, { playsinline: true });
  const layer = element("div", { class: "markers" });
  const stage = element("div", { class: "stage" }, [media, layer]);
  const list = element("div", { class: "point-list" });
  body.append(stage);
  if (kind === "video") body.append(videoControls(media));

  let points = (previous?.points ?? question.default?.points ?? []).map((point) => ({ ...point }));
  const natural = () =>
    kind === "video" ? [media.videoWidth, media.videoHeight] : [media.naturalWidth, media.naturalHeight];

  const draw = () => {
    layer.replaceChildren(
      ...points.map((point, index) => {
        const marker = element("div", {
          class: "marker",
          style: `left: ${point.rx * 100}%; top: ${point.ry * 100}%`,
          title: `${point.x}, ${point.y}${point.time != null ? ` @ ${formatTime(point.time)}` : ""}`,
        });
        marker.addEventListener("click", (event) => {
          event.stopPropagation();
          points.splice(index, 1);
          draw();
        });
        return marker;
      }),
    );
    list.replaceChildren(
      ...points.map((point) => {
        const item = element("span", {
          class: "point",
          text: `(${point.x}, ${point.y})${point.time != null ? ` @ ${formatTime(point.time)}` : ""}`,
        });
        if (point.time != null) item.addEventListener("click", () => (media.currentTime = point.time));
        return item;
      }),
    );
  };

  stage.addEventListener("click", (event) => {
    const [width, height] = natural();
    if (!width || !height) return;
    const rect = media.getBoundingClientRect();
    const rx = (event.clientX - rect.left) / rect.width;
    const ry = (event.clientY - rect.top) / rect.height;
    if (rx < 0 || rx > 1 || ry < 0 || ry > 1) return;
    const point = {
      x: Math.round(rx * width),
      y: Math.round(ry * height),
      rx: round(rx, 4),
      ry: round(ry, 4),
    };
    if (kind === "video") point.time = round(media.currentTime, 2);
    if (question.multiple) points.push(point);
    else points = [point];
    draw();
  });

  body.append(
    element("div", { class: "tools" }, [
      list,
      button("Clear", () => {
        points = [];
        draw();
      }),
    ]),
  );
  draw();

  return () => {
    if (!points.length) return null;
    const [width, height] = natural();
    return { width, height, points };
  };
}

function renderDraw(segment, body) {
  const { question, id, previous } = segment;
  const canvas = element("canvas", { class: "drawing" });
  const context = canvas.getContext("2d");
  const strokes = [];
  let current = null;
  let image = null;

  const paint = (stroke, from = 0) => {
    context.strokeStyle = stroke.color;
    context.fillStyle = stroke.color;
    context.lineWidth = stroke.width;
    context.lineCap = "round";
    context.lineJoin = "round";
    if (stroke.points.length === 1) {
      const [point] = stroke.points;
      context.beginPath();
      context.arc(point.x, point.y, stroke.width / 2, 0, Math.PI * 2);
      context.fill();
      return;
    }
    context.beginPath();
    context.moveTo(stroke.points[Math.max(from - 1, 0)].x, stroke.points[Math.max(from - 1, 0)].y);
    for (const point of stroke.points.slice(from)) context.lineTo(point.x, point.y);
    context.stroke();
  };
  const repaint = () => {
    context.clearRect(0, 0, canvas.width, canvas.height);
    if (image) context.drawImage(image, 0, 0, canvas.width, canvas.height);
    else {
      // A blank sheet is white: a transparent PNG shows as black in many viewers.
      context.fillStyle = "#fff";
      context.fillRect(0, 0, canvas.width, canvas.height);
    }
    for (const stroke of strokes) paint(stroke);
  };
  const size = () => {
    canvas.width = image ? image.naturalWidth : (question.width ?? 800);
    canvas.height = image ? image.naturalHeight : (question.height ?? 600);
    repaint();
  };

  // An earlier drawing already has the background in it, so it is the
  // background now, and new strokes go on top.
  const background = previous?.path ?? question.src;
  if (background) {
    image = new Image();
    // Without this the canvas is tainted by the picture (it comes from another
    // origin) and cannot be exported.
    image.crossOrigin = "anonymous";
    image.addEventListener("load", size);
    image.addEventListener("error", () => {
      statusLine.textContent = `Could not load ${background}`;
      image = null;
      size();
    });
    image.src = mediaUrl(background);
  }
  size();

  const color = element("input", { type: "color", value: "#e0245e", title: "Pen colour" });
  const width = element("select", { title: "Pen width" });
  for (const option of [2, 4, 8, 16]) width.append(element("option", { value: option, text: `${option}px`, selected: option === 4 }));

  const position = (event) => {
    const rect = canvas.getBoundingClientRect();
    return {
      x: ((event.clientX - rect.left) * canvas.width) / rect.width,
      y: ((event.clientY - rect.top) * canvas.height) / rect.height,
    };
  };
  canvas.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    canvas.setPointerCapture(event.pointerId);
    current = { color: color.value, width: Number(width.value), points: [position(event)] };
    strokes.push(current);
    paint(current);
  });
  canvas.addEventListener("pointermove", (event) => {
    if (!current) return;
    current.points.push(position(event));
    paint(current, current.points.length - 1);
  });
  for (const event of ["pointerup", "pointercancel"]) canvas.addEventListener(event, () => (current = null));

  body.append(
    element("div", { class: "tools" }, [
      color,
      width,
      button("Undo", () => {
        strokes.pop();
        repaint();
      }),
      button("Clear", () => {
        strokes.length = 0;
        repaint();
      }),
    ]),
    canvas,
  );

  return {
    collect: () => {
      if (strokes.length) return { width: canvas.width, height: canvas.height };
      // Nothing added: the earlier picture is still the answer, as it is on disk.
      return previous?.path ? previous : null;
    },
    finalize: async (answer) => {
      if (!strokes.length) return answer;
      const path = await invoke("save_drawing", { id, png: canvas.toDataURL("image/png") });
      return { path, ...answer };
    },
  };
}

function renderColor(segment, body) {
  const { question, id, previous } = segment;
  const valid = (value) => /^#[0-9a-f]{6}$/i.test(value);
  const initial = String(previous ?? question.default ?? "");
  const picker = element("input", { type: "color", value: valid(initial) ? initial : "#000000" });
  const text = element("input", { type: "text", class: "color-text", placeholder: "#rrggbb", value: initial });
  picker.addEventListener("input", () => (text.value = picker.value));
  text.addEventListener("input", () => {
    if (valid(text.value.trim())) picker.value = text.value.trim().toLowerCase();
  });
  body.append(element("div", { class: "tools" }, [picker, text]));
  return () => {
    const value = text.value.trim();
    if (!value) return null;
    if (!valid(value)) throw new Error(`${id}: a colour is written as #rrggbb`);
    return value.toLowerCase();
  };
}

function renderRange(segment, body) {
  const { question, id, previous } = segment;
  if (!question.src || mediaKind(question.src) === "image") {
    return unusable(body, "a range question needs an audio or video `src`");
  }
  const media = mediaElement(question.src, { controls: true });
  const fill = element("div", { class: "fill" });
  const bar = element("div", { class: "span-bar" }, [fill]);
  const initial = previous ?? question.default ?? {};

  const fields = {};
  const rows = ["start", "end"].map((name) => {
    const field = element("input", { type: "number", class: "seconds", min: 0, step: 0.1 });
    if (initial[name] != null) field.value = initial[name];
    field.addEventListener("input", showSpan);
    fields[name] = field;
    const nudge = (delta) =>
      button(`${delta > 0 ? "+" : ""}${delta}`, () => {
        field.value = round(Math.max(0, (Number(field.value) || 0) + delta), 1);
        media.currentTime = Number(field.value);
        showSpan();
      });
    return element("div", { class: "range-row" }, [
      element("span", { class: "range-label", text: name }),
      field,
      element("span", { text: "s" }),
      button("Use current time", () => {
        field.value = round(media.currentTime, 1);
        showSpan();
      }),
      nudge(-0.5),
      nudge(-0.1),
      nudge(0.1),
      nudge(0.5),
    ]);
  });

  function showSpan() {
    const duration = media.duration;
    const start = Number(fields.start.value);
    const end = Number(fields.end.value);
    if (!Number.isFinite(duration) || !duration || fields.start.value === "" || fields.end.value === "" || end <= start) {
      fill.style.display = "none";
      return;
    }
    fill.style.display = "";
    fill.style.left = `${(Math.min(start, duration) / duration) * 100}%`;
    fill.style.width = `${((Math.min(end, duration) - Math.min(start, duration)) / duration) * 100}%`;
  }
  media.addEventListener("loadedmetadata", showSpan);

  let stopAt = null;
  media.addEventListener("timeupdate", () => {
    if (stopAt != null && media.currentTime >= stopAt) {
      media.pause();
      stopAt = null;
    }
  });
  media.addEventListener("pause", () => (stopAt = null));
  const play = button("Play the range", () => {
    if (fields.start.value === "" || fields.end.value === "") return;
    media.currentTime = Number(fields.start.value);
    stopAt = Number(fields.end.value);
    media.play();
  });

  body.append(media, bar, ...rows, element("div", { class: "tools" }, [play]));

  return () => {
    if (fields.start.value === "" && fields.end.value === "") return null;
    if (fields.start.value === "" || fields.end.value === "") throw new Error(`${id}: both start and end are needed`);
    const start = Number(fields.start.value);
    const end = Number(fields.end.value);
    if (!Number.isFinite(start) || !Number.isFinite(end) || start < 0) {
      throw new Error(`${id}: start and end are seconds from the beginning`);
    }
    if (!(end > start)) throw new Error(`${id}: end must be after start`);
    if (!Number.isFinite(media.duration)) throw new Error(`${id}: the media has not loaded, so the range cannot be checked`);
    if (end > media.duration + 0.05) throw new Error(`${id}: end is past the end of the media (${formatTime(media.duration)})`);
    return { start, end, duration: round(media.duration, 2) };
  };
}

// A question whose YAML cannot be turned into an input. It is shown as such,
// and if it is required it blocks the submit, as any unanswered question would.
function unusable(body, problem) {
  body.append(element("p", { class: "unsupported", text: problem }));
  return { unusable: problem };
}

const renderers = {
  choice: renderChoice,
  text: renderText,
  "media-choice": renderMediaChoice,
  point: renderPoint,
  draw: renderDraw,
  color: renderColor,
  range: renderRange,
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
  const result = render(segment, body);
  const { collect, finalize } =
    typeof result === "function"
      ? { collect: result }
      : result.unusable
        ? {
            collect: () => {
              if (segment.question.required) throw new Error(`${segment.id}: ${result.unusable}`);
              return null;
            },
          }
        : result;
  collectors.push({ id: segment.id, required: segment.question.required, card, collect, finalize });
  return card;
}

async function load() {
  const view = await invoke("document");
  mediaBase = view.media_base;
  document.getElementById("path").textContent = view.path;
  form.replaceChildren(
    ...view.segments.map((segment) => (segment.kind === "prose" ? trustedHtml(segment.html) : renderQuestion(segment))),
  );
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  submitButton.disabled = true;
  statusLine.textContent = "";
  try {
    const answers = {};
    const pending = [];
    const problems = [];
    for (const collector of collectors) {
      const { id, required, card, collect, finalize } = collector;
      let value = null;
      try {
        value = collect();
      } catch (error) {
        problems.push(error.message ?? String(error));
        card.classList.add("missing");
        continue;
      }
      card.classList.toggle("missing", required && value == null);
      if (value == null) {
        if (required) problems.push(`${id} is required`);
        continue;
      }
      answers[id] = value;
      if (finalize) pending.push(async () => (answers[id] = await finalize(value)));
    }
    if (problems.length) {
      statusLine.textContent = problems.join(" · ");
      form.querySelector(".missing")?.scrollIntoView({ behavior: "smooth", block: "center" });
      return;
    }
    for (const run of pending) await run();
    await invoke("submit", { answers });
  } catch (error) {
    statusLine.textContent = String(error);
  } finally {
    submitButton.disabled = false;
  }
});

document.getElementById("cancel").addEventListener("click", () => invoke("cancel"));

// Links open in the browser; the window itself never navigates away.
document.addEventListener("click", (event) => {
  const link = event.target.closest("a[href]");
  if (!link) return;
  const href = link.getAttribute("href");
  if (href.startsWith("#")) return;
  event.preventDefault();
  if (/^https?:\/\//i.test(href)) {
    invoke("open_link", { url: href }).catch((error) => (statusLine.textContent = String(error)));
  }
});

load().catch((error) => {
  statusLine.textContent = String(error);
});
