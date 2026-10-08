// The sift window. Every action calls a Rust command by name with `invoke`
// (see app/src-tauri/src/main.rs), which answers with plain data. Pages are
// built with `el` below rather than innerHTML, so a file named like HTML is
// shown as text, never run as markup.

const { invoke } = window.__TAURI__.core;
const $ = (id) => document.getElementById(id);

/** el("div", { class: "row" }, child, "text", ...) -> an element. */
function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (value === false || value == null) continue;
    if (key.startsWith("on")) node.addEventListener(key.slice(2), value);
    else node.setAttribute(key, value === true ? "" : value);
  }
  for (const child of children.flat()) {
    if (child == null || child === false) continue;
    node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return node;
}

let toastTimer;
function toast(message, bad = false) {
  const box = $("toast");
  box.textContent = message;
  box.classList.toggle("bad", bad);
  box.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (box.hidden = true), bad ? 6000 : 3500);
}

/** Runs a command; shows its error as a red toast instead of failing silently. */
async function call(command, args) {
  try {
    return await invoke(command, args);
  } catch (error) {
    toast(String(error), true);
    throw error;
  }
}

// ---------------------------------------------------------------- navigation

let current = "review";
function show(view) {
  current = view;
  document.querySelectorAll(".nav button").forEach((b) => b.classList.toggle("active", b.dataset.view === view));
  document.querySelectorAll(".view").forEach((v) => (v.hidden = v.id !== view));
  ({ review: loadReview, activity: loadActivity, folders: loadFolders, rules: loadRules, test: () => {} })[view]();
}
document.querySelectorAll(".nav button").forEach((b) => b.addEventListener("click", () => {
  if (current === "rules" && rulesDirty() && !confirm("Leave without saving your rule changes?")) return;
  show(b.dataset.view);
}));

// -------------------------------------------------------------------- status

async function loadStatus() {
  const status = await invoke("status");
  document.querySelectorAll(".segmented button").forEach((b) =>
    b.setAttribute("aria-checked", String(b.dataset.mode === status.mode)));
  $("watching").checked = status.watching;
  $("waiting-count").textContent = status.waiting;
  $("waiting-count").hidden = status.waiting === 0;
  $("rules-path").replaceChildren("File: ", el("code", {}, status.rules_path));
  $("config-error").hidden = !status.error;
  $("config-error").textContent = status.error ? `The rules file has a problem, so sift can't sort until it's fixed (see Rules):\n${status.error}` : "";
  $("watch-hint").textContent = status.mode === "move"
    ? "Moves new downloads as they arrive, from login on."
    : "Lines up new downloads for review as they arrive, from login on.";
}

document.querySelectorAll(".segmented button").forEach((b) => b.addEventListener("click", async () => {
  await call("set_mode", { mode: b.dataset.mode });
  await loadStatus();
  toast(b.dataset.mode === "move" ? "sift will move files without asking." : "sift will ask before moving anything.");
}));

$("watching").addEventListener("change", async (event) => {
  const on = event.target.checked;
  try {
    await call("set_watching", { on });
    toast(on ? "Watching Downloads and Desktop. macOS may ask to let sift use them." : "Stopped watching.");
  } catch {
    event.target.checked = !on;
  }
});

// -------------------------------------------------------------------- review

/** "~/Documents/School/COSC 490/Lab.pdf" -> folder and name, for display. */
function splitPath(path) {
  const i = path.lastIndexOf("/");
  return i < 0 ? ["", path] : [path.slice(0, i), path.slice(i + 1)];
}

function kindOf(name) {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1, dot + 5) : "file";
}

const SHOWN = 8; // rows per group before "Show all"
const expanded = new Set();

async function loadReview() {
  const items = await call("suggestions");
  await loadStatus();
  $("review-sub").textContent = items.length
    ? `${items.length} file${items.length === 1 ? "" : "s"} waiting for your OK.`
    : "Files sift wants to move, waiting for your OK.";
  $("review-empty").hidden = items.length > 0;

  // Group by rule, keeping the order rules first appear in.
  const groups = new Map();
  for (const item of items) {
    if (!groups.has(item.rule)) groups.set(item.rule, []);
    groups.get(item.rule).push(item);
  }
  $("groups").replaceChildren(...[...groups].map(([rule, list]) => group(rule, list)));
}

function group(rule, list) {
  const trash = list[0].trash;
  const all = list.map((i) => i.path);
  const open = expanded.has(rule) || list.length <= SHOWN + 2;
  const rows = (open ? list : list.slice(0, SHOWN)).map(row);
  return el("div", { class: "group" },
    el("div", { class: "group-head" },
      el("h2", {}, rule, " ", el("span", { class: "count" }, `· ${list.length}`)),
      el("button", { class: "btn small quiet", onclick: () => decide("dismiss", all) }, "Leave All"),
      el("button", { class: `btn small ${trash ? "danger" : "primary"}`, onclick: () => decide("approve", all) },
        trash ? `Trash All ${list.length}` : `Move All ${list.length}`)),
    el("ul", { class: "rows" }, rows),
    !open && el("button", { class: "more", onclick: () => { expanded.add(rule); loadReview(); } },
      `Show all ${list.length}`));
}

function row(item) {
  const [folder, file] = splitPath(item.to);
  return el("li", { class: "row" },
    el("div", { class: "kind", "aria-hidden": "true" }, kindOf(item.name)),
    el("div", { class: "who" },
      el("div", { class: "name clickable", title: "Show in Finder", onclick: () => call("reveal", { path: item.path }) }, item.name),
      el("div", { class: "path", title: item.to },
        item.from, el("span", { class: "arrow" }, "→"),
        item.trash ? el("span", { class: "dest" }, "the Trash")
                   : [folder + "/", el("span", { class: "dest" }, file)],
        item.tags.length ? `  ·  ${item.tags.join(", ")}` : "")),
    el("div", { class: "actions" },
      el("button", { class: "btn small quiet", onclick: () => decide("dismiss", [item.path]) }, "Leave"),
      el("button", { class: `btn small ${item.trash ? "danger" : ""}`, onclick: () => decide("approve", [item.path]) },
        item.trash ? "Trash" : "Move")));
}

async function decide(command, paths) {
  const problems = await call(command, { paths });
  if (command === "approve") {
    if (problems && problems.length) toast(problems.join("\n"), true);
    else toast(`${paths.length === 1 ? "Done" : `${paths.length} files done`}. Undo is in Activity.`);
  }
  loadReview();
}

$("scan").addEventListener("click", async () => {
  const added = await call("scan");
  const status = await invoke("status");
  toast(status.mode === "move"
    ? "Sorted what was waiting in Downloads and Desktop."
    : added ? `${added} new file${added === 1 ? "" : "s"} to review.` : "Nothing new to sort.");
  loadReview();
});

// ------------------------------------------------------------------ activity

async function loadActivity() {
  const moves = await call("history", { limit: 200 });
  $("activity-empty").hidden = moves.length > 0;
  $("moves").hidden = moves.length === 0;
  $("moves").replaceChildren(...moves.map((m) => {
    const where = m.to === "the Trash" ? "the Trash" : splitPath(m.to)[0];
    return el("li", { class: `move${m.undone ? " undone" : ""}` },
      el("div", { class: "when" }, m.time),
      el("div", { class: "who" },
        el("div", { class: "name clickable", title: "Show in Finder",
          onclick: () => call("reveal", { path: m.undone ? `${m.from}/${m.name}` : m.to === "the Trash" ? m.from : m.to }) }, m.name),
        el("div", { class: "path" }, m.from, el("span", { class: "arrow" }, "→"), el("span", { class: "dest" }, where),
          el("span", { class: "rule-tag" }, `  ·  ${m.rule}`))),
      m.undone
        ? el("span", { class: "rule-tag" }, "Undone")
        : el("button", { class: "btn small", onclick: async () => { toast(await call("undo", { index: m.index })); loadActivity(); } }, "Undo"));
  }));
}

// ----------------------------------------------------------- where files go

const SUBFOLDERS = [["", "No subfolders"], ["{year}", "By year"], ["{year}-{month}", "By month"], ["{course}", "By course"]];

/** A folder picker that starts in `from` ("~/Pictures" style paths work). */
async function pickFolder(from) {
  return window.__TAURI__.dialog.open({ directory: true, multiple: false, defaultPath: from || undefined });
}

async function loadFolders() {
  const rules = await call("rule_list");
  $("rule-cards").replaceChildren(...rules.map(ruleCard));
}

function ruleCard(rule) {
  const toggle = el("input", { type: "checkbox", "aria-label": `${rule.name} on` });
  toggle.checked = rule.enabled;
  toggle.addEventListener("change", async () => {
    await call("set_rule_enabled", { name: rule.name, on: toggle.checked });
    loadFolders();
  });

  let destination;
  if (rule.trash) {
    destination = el("div", { class: "trash-note" }, "Goes to the Trash (Finder's Put Back still works).");
  } else {
    // Subfolder choices, plus this rule's own pattern if it's something else.
    const choices = SUBFOLDERS.some(([value]) => value === rule.subfolders) ? SUBFOLDERS
      : [...SUBFOLDERS, [rule.subfolders, `Custom: ${rule.subfolders}`]];
    const select = el("select", { "aria-label": `${rule.name} subfolders` },
      choices.map(([value, label]) => el("option", { value, selected: value === rule.subfolders }, label)));
    select.addEventListener("change", async () => {
      await call("set_rule_folder", { name: rule.name, folder: rule.folder, subfolders: select.value });
      toast(`${rule.name}: ${select.value ? select.selectedOptions[0].textContent.toLowerCase() : "no subfolders"}.`);
      loadFolders();
    });
    destination = el("div", { class: "rule-dest" },
      el("code", { class: "path-box clickable", title: "Show in Finder", onclick: () => call("reveal", { path: rule.folder }) },
        rule.folder + (rule.subfolders ? `/${rule.subfolders}` : "")),
      el("button", { class: "btn small", onclick: async () => {
        const folder = await pickFolder(rule.folder);
        if (!folder) return;
        await call("set_rule_folder", { name: rule.name, folder, subfolders: rule.subfolders });
        toast(`${rule.name} now go to ${folder.replace(/^\/Users\/[^/]+/, "~")}.`);
        loadFolders();
      } }, "Choose Folder…"),
      select);
  }

  return el("li", { class: `rule-card${rule.enabled ? "" : " off"}` },
    el("div", { class: "rule-top" },
      el("label", { class: "switch", title: rule.enabled ? "On" : "Off" }, toggle,
        el("span", { class: "track" }, el("span", { class: "thumb" }))),
      el("div", { class: "who" }, el("h2", {}, rule.name), el("div", { class: "path" }, rule.summary)),
      el("button", { class: "btn link danger", onclick: async () => {
        if (!confirm(`Remove the rule \u201C${rule.name}\u201D? Files already sorted stay where they are.`)) return;
        await call("remove_rule", { name: rule.name });
        loadFolders();
      } }, "Remove")),
    destination);
}

// The new-rule form.
const picked = new Set();
let newFolder = "";
async function openNewRule() {
  const kinds = await call("kinds");
  picked.clear();
  newFolder = "";
  $("new-folder").textContent = "Choose a folder";
  $("kind-chips").replaceChildren(...kinds.map((kind) => {
    const chip = el("button", { type: "button", class: "chip", "aria-pressed": "false" }, kind);
    chip.addEventListener("click", () => {
      picked.has(kind) ? picked.delete(kind) : picked.add(kind);
      chip.setAttribute("aria-pressed", String(picked.has(kind)));
    });
    return chip;
  }));
  $("new-rule").reset();
  $("new-rule-error").hidden = true;
  $("new-rule").hidden = false;
  $("new-rule").elements.name.focus();
}
$("add-rule").addEventListener("click", openNewRule);
$("cancel-rule").addEventListener("click", () => ($("new-rule").hidden = true));
$("new-folder-pick").addEventListener("click", async () => {
  const folder = await pickFolder();
  if (folder) {
    newFolder = folder;
    $("new-folder").textContent = folder.replace(/^\/Users\/[^/]+/, "~");
  }
});
$("new-rule").addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = event.target.elements;
  const rule = { name: form.name.value, kinds: [...picked], from: form.from.value, folder: newFolder, subfolders: form.subfolders.value };
  try {
    await invoke("add_rule", { rule });
    $("new-rule").hidden = true;
    toast(`Added \u201C${rule.name.trim()}\u201D.`);
    loadFolders();
  } catch (error) {
    $("new-rule-error").textContent = String(error);
    $("new-rule-error").hidden = false;
  }
});

// --------------------------------------------------------------------- rules

let savedRules = "";
const rulesDirty = () => $("rules-text").value !== savedRules;

async function loadRules() {
  savedRules = await call("rules_text");
  $("rules-text").value = savedRules;
  $("rules-error").hidden = true;
  updateRuleButtons();
}

function updateRuleButtons() {
  $("save-rules").disabled = !rulesDirty();
  $("revert-rules").disabled = !rulesDirty();
}

$("rules-text").addEventListener("input", updateRuleButtons);
// Tab inserts two spaces instead of leaving the editor.
$("rules-text").addEventListener("keydown", (event) => {
  if (event.key === "Tab") {
    event.preventDefault();
    document.execCommand("insertText", false, "  ");
  }
});

async function saveRules() {
  if (!rulesDirty()) return;
  try {
    await invoke("save_rules", { text: $("rules-text").value });
    savedRules = $("rules-text").value;
    $("rules-error").hidden = true;
    updateRuleButtons();
    loadStatus();
    toast("Rules saved.");
  } catch (error) {
    // The file wasn't changed; say what's wrong right above the editor.
    $("rules-error").textContent = `Not saved: ${error}`;
    $("rules-error").hidden = false;
  }
}
$("save-rules").addEventListener("click", saveRules);
$("revert-rules").addEventListener("click", loadRules);
$("open-rules").addEventListener("click", () => call("open_rules"));
document.addEventListener("keydown", (event) => {
  if ((event.metaKey || event.ctrlKey) && event.key === "s" && current === "rules") {
    event.preventDefault();
    saveRules();
  }
});

// --------------------------------------------------------------- test a file

async function explain(path) {
  const e = await call("explain", { path });
  const facts = [
    ["Type", e.kind],
    ["Arrived", e.arrived],
    ["Unused for", `${e.unused_days} day${e.unused_days === 1 ? "" : "s"}`],
    ["Downloaded from", e.from.length ? e.from.join(", ") : "Not a download"],
    ["Course", e.course ?? "None found"],
    ["Tags", e.tags.length ? e.tags.join(", ") : "None"],
    ["Text", e.text_preview ? `${e.text_preview}…` : "None read"],
  ];
  $("explanation").replaceChildren(el("div", { class: "card" },
    el("h2", {}, e.name),
    el("dl", { class: "facts" }, facts.flatMap(([k, v]) => [el("dt", {}, k), el("dd", {}, v)])),
    e.unnamed_course && nameCourse(e.unnamed_course, path),
    el("div", { class: `outcome${e.outcome ? "" : " none"}` },
      e.outcome ? ["Would go to ", el("strong", {}, e.outcome)] : "No rule matches, so it stays where it is."),
    el("h2", {}, "Rules, top to bottom"),
    el("ul", { class: "verdicts" }, e.verdicts.map((v) =>
      el("li", { class: `verdict ${v.matched ? "yes" : "no"}` },
        el("span", { class: "mark" }, v.matched ? "✓" : "✗"),
        el("div", {}, el("div", {}, v.rule), v.reason && el("div", { class: "why" }, v.reason)))))));
}

/** A Canvas course with a number but no name: name it right here. */
function nameCourse(number, path) {
  const input = el("input", { placeholder: "COSC 360", "aria-label": "Course code" });
  const save = async () => {
    const code = input.value.trim();
    if (!code) return;
    let text = await call("rules_text");
    const line = `"${number}" = "${code.replace(/"/g, "")}"`;
    // Add it under [course_names], making that section if it isn't there.
    text = /^\[course_names\]\s*$/m.test(text)
      ? text.replace(/^\[course_names\]\s*$/m, (m) => `${m}\n${line}`)
      : text.replace(/^\[\[rule\]\]/m, `[course_names]\n${line}\n\n[[rule]]`);
    try {
      await invoke("save_rules", { text });
      toast(`Canvas course ${number} is now ${code}.`);
      explain(path);
    } catch (error) {
      toast(`Couldn't save: ${error}`, true);
    }
  };
  input.addEventListener("keydown", (event) => event.key === "Enter" && save());
  return el("div", { class: "name-course" },
    `Canvas course ${number} has no name yet. Its code:`, input,
    el("button", { class: "btn small primary", onclick: save }, "Save"));
}

$("choose").addEventListener("click", async () => {
  const path = await window.__TAURI__.dialog.open({ multiple: false, directory: false });
  if (path) explain(path);
});

// Files dropped on the window (Tauri gives real paths, unlike a web page).
window.__TAURI__.webview.getCurrentWebview().onDragDropEvent((event) => {
  const zone = $("drop");
  const type = event.payload.type;
  if (type === "enter" || type === "over") {
    if (current !== "test") show("test");
    zone.classList.add("over");
  } else if (type === "drop") {
    zone.classList.remove("over");
    if (event.payload.paths.length) explain(event.payload.paths[0]);
  } else {
    zone.classList.remove("over");
  }
});

// --------------------------------------------------------------------- start

// Refresh when the window comes back to the front: the watcher may have lined
// up new files meanwhile.
window.addEventListener("focus", () => {
  loadStatus();
  if (current === "review") loadReview();
  if (current === "activity") loadActivity();
});
show("review");
