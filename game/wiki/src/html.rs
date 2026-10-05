use crate::Page;

const HEAD: &str = r####"<!DOCTYPE html>
<html lang="en">
<head>
<script>
try {
  if (localStorage.getItem("wiki-theme") === "light") {
    document.documentElement.setAttribute("data-theme", "light");
  }
} catch (err) {}
</script>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Scripting Wiki</title>
<style>
/*__WIKI_FONTS__*/
:root {
  color-scheme: dark;
  --paper: #0c0d10;
  --nav: #0f1013;
  --card: #14161a;
  --card-2: #181a1f;
  --code-bg: #111317;
  --code-ink: #e6e2da;
  --ink: #ece8e1;
  --soft: #c4c0b7;
  --muted: #86837c;
  --heading: #a7a39a;
  --line: #24262b;
  --row: #1b1d22;
  --accent: #d6aa62;
  --accent-2: #ebc886;
  --accent-soft: rgba(214, 170, 98, 0.12);
  --glow: rgba(214, 170, 98, 0.07);
  --hover: rgba(255, 255, 255, 0.045);
  --type: #8fb9d8;
  --kw: #d49bb0;
  --str: #a3c99c;
  --num: #e3ae72;
  --com: #6e6c66;
  --dot-client: #6aa4e8;
  --dot-server: #e0a458;
  --dot-menu: #b58be0;
  --warn-bg: rgba(224, 164, 88, 0.10);
  --warn-ink: #e8b872;
  --bad-bg: rgba(226, 98, 98, 0.10);
  --bad-ink: #ec8d8d;
  --shadow: rgba(0, 0, 0, 0.45);
  --sans: "Manrope", "Segoe UI", system-ui, sans-serif;
  --serif: "Newsreader", Georgia, "Times New Roman", serif;
  --mono: "JetBrains Mono", ui-monospace, Menlo, Consolas, monospace;
}
html[data-theme="light"] {
  color-scheme: light;
  --paper: #f8f5ef;
  --nav: #f3efe7;
  --card: #fffdf9;
  --card-2: #fbf8f2;
  --code-bg: #f1ece1;
  --code-ink: #26231e;
  --ink: #1d1b17;
  --soft: #46423a;
  --muted: #7b766b;
  --heading: #6a655b;
  --line: #e2dccf;
  --row: #ebe5d8;
  --accent: #98681b;
  --accent-2: #b8862e;
  --accent-soft: rgba(152, 104, 27, 0.10);
  --glow: rgba(184, 134, 46, 0.09);
  --hover: rgba(40, 30, 10, 0.05);
  --type: #2c6a97;
  --kw: #a2456c;
  --str: #3f7a3a;
  --num: #a8631a;
  --com: #9a958a;
  --dot-client: #2f74d0;
  --dot-server: #c0771c;
  --dot-menu: #8554c0;
  --warn-bg: rgba(192, 119, 28, 0.10);
  --warn-ink: #8a5712;
  --bad-bg: rgba(190, 70, 70, 0.09);
  --bad-ink: #a23838;
  --shadow: rgba(60, 40, 10, 0.10);
}
* { box-sizing: border-box; }
html, body { margin: 0; padding: 0; }
html { scroll-behavior: smooth; }
body {
  font-family: var(--sans);
  font-size: 15px;
  line-height: 1.6;
  color: var(--ink);
  background: var(--paper);
  font-synthesis: none;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  text-rendering: optimizeLegibility;
}
button, input { font: inherit; }
::selection { background: var(--accent-soft); color: var(--ink); }
:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
#app { display: flex; align-items: flex-start; min-height: 100vh; }

aside {
  width: 304px;
  flex: none;
  position: sticky;
  top: 0;
  height: 100vh;
  overflow: auto;
  background: var(--nav);
  border-right: 1px solid var(--line);
  color: var(--soft);
  padding: 30px 18px 40px 20px;
  scrollbar-width: thin;
  scrollbar-color: var(--line) transparent;
}
aside::-webkit-scrollbar { width: 8px; }
aside::-webkit-scrollbar-thumb { background: var(--line); border-radius: 99px; }
.brand-row {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 8px;
  padding: 0 6px 22px;
}
.brand { padding: 0; }
.brand strong {
  display: flex;
  align-items: center;
  gap: 11px;
  color: var(--ink);
  font-family: var(--serif);
  font-weight: 400;
  font-size: 28px;
  line-height: 1;
  letter-spacing: -0.01em;
}
.brand strong::before {
  content: "";
  width: 8px;
  height: 8px;
  background: var(--accent);
  transform: rotate(45deg);
  flex: none;
}
.brand span {
  display: block;
  margin: 9px 0 0 19px;
  color: var(--muted);
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.24em;
  text-transform: uppercase;
}
#theme {
  border: 1px solid var(--line);
  background: transparent;
  color: var(--muted);
  border-radius: 999px;
  padding: 5px 11px;
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  cursor: pointer;
  transition: color 0.15s, border-color 0.15s;
}
#theme:hover { color: var(--accent); border-color: var(--accent); }
.search-wrap { position: relative; margin: 0 4px; }
#search {
  width: 100%;
  border: 1px solid var(--line);
  background: var(--card);
  color: var(--ink);
  border-radius: 10px;
  padding: 10px 34px 10px 13px;
  font-size: 14px;
  outline: none;
  transition: border-color 0.15s, box-shadow 0.15s;
}
#search::placeholder { color: var(--muted); }
#search:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
.search-wrap kbd {
  position: absolute;
  right: 10px;
  top: 50%;
  transform: translateY(-50%);
  font-family: var(--mono);
  font-size: 11px;
  color: var(--muted);
  border: 1px solid var(--line);
  border-radius: 5px;
  padding: 0 6px;
  line-height: 18px;
  pointer-events: none;
}
#search:focus ~ kbd, #search:not(:placeholder-shown) ~ kbd { display: none; }
.group { margin-top: 28px; }
.group h2 {
  display: flex;
  align-items: center;
  gap: 12px;
  margin: 0 6px 8px;
  font-size: 10px;
  letter-spacing: 0.22em;
  text-transform: uppercase;
  color: var(--muted);
  font-weight: 600;
}
.group h2::after { content: ""; flex: 1; height: 1px; background: var(--line); }
.nav-link {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  color: var(--soft);
  text-decoration: none;
  border-radius: 7px;
  padding: 5px 10px;
  font-size: 14px;
  transition: background 0.12s, color 0.12s;
}
.nav-link:hover { background: var(--hover); color: var(--ink); }
.nav-link.active { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
.nav-link.active::before {
  content: "";
  position: absolute;
  left: 0;
  top: 7px;
  bottom: 7px;
  width: 2px;
  border-radius: 2px;
  background: var(--accent);
}
.branch:not(.open) > .kids { display: none; }
.kids {
  margin: 2px 0 8px 14px;
  padding-left: 8px;
  border-left: 1px solid var(--line);
}
.kids .nav-link { font-size: 13px; color: var(--muted); padding: 3px 10px; }
.kids .nav-link:hover { color: var(--ink); }
.kids .nav-link.active { color: var(--accent); }
.folder-name {
  display: block;
  margin: 10px 10px 2px;
  font-size: 9.5px;
  letter-spacing: 0.2em;
  text-transform: uppercase;
  color: var(--muted);
  font-weight: 600;
  opacity: 0.75;
}
h4.folder-title {
  margin: 20px 0 6px;
  font-size: 10.5px;
  font-weight: 600;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: var(--muted);
}

main {
  flex: 1;
  min-width: 0;
  padding: 60px 72px 140px;
  background: radial-gradient(900px 420px at 18% -8%, var(--glow), transparent 70%) no-repeat;
}
article {
  max-width: 820px;
  animation: rise 0.35s cubic-bezier(0.2, 0.7, 0.2, 1) both;
}
article.home { max-width: none; }
@keyframes rise {
  from { opacity: 0; transform: translateY(8px); }
  to { opacity: 1; transform: none; }
}
@media (prefers-reduced-motion: reduce) {
  article { animation: none; }
  html { scroll-behavior: auto; }
}
.crumbs { margin: 0 0 30px; color: var(--muted); font-size: 12.5px; letter-spacing: 0.02em; }
.crumbs a { color: var(--muted); text-decoration: none; transition: color 0.12s; }
.crumbs a:hover { color: var(--accent); }
.crumbs span { margin: 0 3px; opacity: 0.55; }
.title-row {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 24px;
  flex-wrap: wrap;
}
.eyebrow {
  margin: 0 0 12px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.22em;
  text-transform: uppercase;
  color: var(--accent);
}
h1 {
  margin: 0;
  font-family: var(--serif);
  font-weight: 400;
  font-size: 48px;
  line-height: 1.06;
  letter-spacing: -0.025em;
  overflow-wrap: anywhere;
}
.realms { display: flex; gap: 6px; flex: none; padding-bottom: 8px; flex-wrap: wrap; }
.realm, .lang, .access {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  font-size: 10.5px;
  font-weight: 600;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--muted);
  border: 1px solid var(--line);
  border-radius: 999px;
  padding: 3px 10px 3px 8px;
  flex: none;
  line-height: 1.5;
}
.realm::before {
  content: "";
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--dot);
  flex: none;
}
.realm.client { --dot: var(--dot-client); }
.realm.server { --dot: var(--dot-server); }
.realm.menu { --dot: var(--dot-menu); }
.access { padding-left: 10px; }
.access.internal { color: var(--accent); border-color: var(--accent); }
.nav-link .realm {
  font-size: 0;
  gap: 0;
  padding: 0;
  border: 0;
  width: 7px;
  height: 7px;
}
.nav-link .realm::before { width: 6px; height: 6px; opacity: 0.85; }
.badges { display: flex; gap: 5px; align-items: center; flex: none; }
.member-name .realm { font-size: 0; gap: 0; padding: 0; border: 0; }
pre.sig, pre.code {
  font-family: var(--mono);
  overflow-x: auto;
  scrollbar-width: thin;
  scrollbar-color: var(--line) transparent;
}
pre.sig {
  margin: 30px 0 0;
  padding: 18px 22px;
  background: var(--code-bg);
  color: var(--code-ink);
  border: 1px solid var(--line);
  border-left: 2px solid var(--accent);
  border-radius: 4px 12px 12px 4px;
  font-size: 14px;
  line-height: 1.65;
}
pre.sig .type, pre.sig a.type { color: var(--type); text-decoration: none; }
pre.sig a.type:hover { text-decoration: underline; text-underline-offset: 3px; }
.summary {
  margin: 28px 0 0;
  max-width: 64ch;
  font-family: var(--serif);
  font-size: 21px;
  line-height: 1.55;
  color: var(--soft);
}
.since { color: var(--muted); font-size: 13px; font-style: italic; font-family: var(--serif); }
h3 {
  display: flex;
  align-items: center;
  gap: 16px;
  margin: 52px 0 16px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.22em;
  text-transform: uppercase;
  color: var(--heading);
}
h3::after { content: ""; flex: 1; height: 1px; background: var(--line); }
article > p:not(.crumbs):not(.summary):not(.since):not(.banner):not(.eyebrow) {
  margin: 0;
  max-width: 68ch;
  color: var(--soft);
  line-height: 1.7;
}
table.args { width: 100%; border-collapse: collapse; }
table.args th {
  text-align: left;
  font-size: 10.5px;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: var(--muted);
  font-weight: 600;
  padding: 0 16px 10px 0;
  border-bottom: 1px solid var(--line);
}
table.args td {
  padding: 13px 16px 13px 0;
  vertical-align: top;
  border-bottom: 1px solid var(--row);
  line-height: 1.55;
  color: var(--soft);
}
table.args tr:last-child td { border-bottom: 0; }
table.ret th:first-child, table.ret td:first-child { width: 1%; white-space: nowrap; padding-right: 56px; }
table.args td.name {
  font-family: var(--mono);
  font-size: 13.5px;
  color: var(--ink);
  white-space: nowrap;
}
em.opt, em.opt {
  font-style: normal;
  font-family: var(--sans);
  font-size: 9.5px;
  font-weight: 600;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  color: var(--muted);
  border: 1px solid var(--line);
  border-radius: 999px;
  padding: 1px 7px;
  margin-left: 8px;
}
a.type { color: var(--type); text-decoration: none; font-family: var(--mono); font-size: 13px; }
a.type:hover { text-decoration: underline; text-underline-offset: 3px; }
span.type { font-family: var(--mono); font-size: 13px; color: var(--type); }
pre.code {
  margin: 0;
  padding: 18px 22px;
  background: var(--code-bg);
  color: var(--code-ink);
  border: 1px solid var(--line);
  border-radius: 12px;
  line-height: 1.7;
  font-size: 13.5px;
}
pre.code .kw { color: var(--kw); }
pre.code .str { color: var(--str); }
pre.code .num { color: var(--num); }
pre.code .com { color: var(--com); font-style: italic; }
.banner {
  padding: 12px 16px;
  border-radius: 4px 10px 10px 4px;
  margin: 24px 0 0;
  font-size: 14px;
  border-left: 2px solid currentColor;
}
.banner.warn { background: var(--warn-bg); color: var(--warn-ink); }
.banner.bad { background: var(--bad-bg); color: var(--bad-ink); }
ul.members, ul.seealso { list-style: none; padding: 0; margin: 0; }
ul.members li {
  display: grid;
  grid-template-columns: minmax(190px, 270px) 1fr;
  gap: 8px 24px;
  padding: 11px 0;
  border-bottom: 1px solid var(--row);
  align-items: baseline;
}
ul.members li:last-child { border-bottom: 0; }
.member-name { display: flex; align-items: center; gap: 9px; flex-wrap: wrap; }
ul.members a {
  font-family: var(--mono);
  font-size: 13.5px;
  color: var(--ink);
  text-decoration: none;
  transition: color 0.12s;
}
ul.members li:hover a { color: var(--accent); }
ul.members li > span:last-child { color: var(--muted); font-size: 14px; }
ul.seealso li { padding: 4px 0; color: var(--muted); }
ul.seealso a {
  color: var(--accent);
  text-decoration: none;
  font-family: var(--mono);
  font-size: 13.5px;
}
ul.seealso a:hover { text-decoration: underline; text-underline-offset: 3px; }

.hero { padding: 24px 0 8px; }
.hero h1 { font-size: 76px; line-height: 1; letter-spacing: -0.035em; }
.hero h1 em { font-style: italic; color: var(--accent); }
.hero .lede {
  margin: 28px 0 0;
  max-width: 54ch;
  font-family: var(--serif);
  font-size: 22px;
  line-height: 1.55;
  color: var(--muted);
}
.hero .rule {
  width: 56px;
  height: 1px;
  margin-top: 36px;
  background: var(--accent);
}
.cards {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(250px, 1fr));
  gap: 16px;
}
.card {
  display: block;
  text-decoration: none;
  color: inherit;
  background: var(--card);
  border: 1px solid var(--line);
  border-radius: 14px;
  padding: 20px 22px 22px;
  transition: transform 0.18s ease, border-color 0.18s ease, box-shadow 0.18s ease;
}
.card:hover {
  transform: translateY(-3px);
  border-color: var(--accent);
  box-shadow: 0 14px 34px -14px var(--shadow);
}
.card strong {
  display: block;
  font-family: var(--serif);
  font-weight: 400;
  font-size: 25px;
  line-height: 1.15;
  letter-spacing: -0.01em;
}
.card em {
  display: block;
  margin-top: 5px;
  color: var(--accent);
  font-style: normal;
  font-size: 10.5px;
  font-weight: 600;
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
.card p { margin: 12px 0 0; color: var(--muted); font-size: 14px; line-height: 1.55; }
#menu {
  display: none;
  position: fixed;
  top: 14px;
  right: 14px;
  z-index: 3;
  border: 1px solid var(--line);
  background: var(--card);
  color: var(--ink);
  border-radius: 999px;
  padding: 8px 16px;
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  cursor: pointer;
}
@media (max-width: 1100px) {
  main { padding: 52px 40px 100px; }
  h1 { font-size: 40px; }
  .hero h1 { font-size: 60px; }
}
@media (max-width: 860px) {
  #menu { display: block; }
  aside {
    position: fixed;
    z-index: 2;
    transform: translateX(-105%);
    transition: transform 0.2s ease;
    box-shadow: 12px 0 40px var(--shadow);
  }
  aside.open { transform: none; }
  main { padding: 72px 20px 64px; }
  ul.members li { grid-template-columns: 1fr; gap: 3px; }
  h1 { font-size: 32px; }
  .hero h1 { font-size: 44px; }
  .hero .lede { font-size: 19px; }
  .summary { font-size: 19px; }
  table.args td.name { white-space: normal; }
}
</style>
</head>
<body>
<div id="app">
<aside id="sidebar">
  <div class="brand-row"><div class="brand"><strong>Wiki</strong><span>Scripting API</span></div><button id="theme" type="button" aria-pressed="true">Light</button></div>
  <div class="search-wrap"><input id="search" type="search" placeholder="Search the reference" autocomplete="off" aria-label="Search"><kbd>/</kbd></div>
  <nav id="nav"></nav>
</aside>
<main id="main"></main>
</div>
<button id="menu" type="button">Menu</button>
<script id="wiki-data" type="application/json">"####;

const TAIL: &str = r####"</script>
<script>
(function () {
  var data = JSON.parse(document.getElementById("wiki-data").textContent);
  var byId = {};
  var search = document.getElementById("search");
  var nav = document.getElementById("nav");
  var main = document.getElementById("main");
  var menu = document.getElementById("menu");
  var themeButton = document.getElementById("theme");
  var aside = document.getElementById("sidebar");
  var idx;

  for (idx = 0; idx < data.length; idx++) {
    byId[data[idx].id] = data[idx];
  }

  function esc(text) {
    return String(text).replace(/[&<>"']/g, function (ch) {
      if (ch === "&") return "&amp;";
      if (ch === "<") return "&lt;";
      if (ch === ">") return "&gt;";
      if (ch === '"') return "&quot;";
      return "&#39;";
    });
  }

  function currentId() {
    var raw = location.hash.replace(/^#/, "");

    if (!raw) {
      return "";
    }

    try {
      return decodeURIComponent(raw);
    } catch (err) {
      return raw;
    }
  }

  function query() {
    return search.value.trim().toLowerCase();
  }

  function hit(page, q) {
    if (!q) {
      return true;
    }

    return (page.id + " " + page.summary).toLowerCase().indexOf(q) !== -1;
  }

  function byName(a, b) {
    if (a.name < b.name) return -1;
    if (a.name > b.name) return 1;
    return 0;
  }

  function ofKind(kind) {
    var out = [];

    for (var itemIdx = 0; itemIdx < data.length; itemIdx++) {
      if (data[itemIdx].kind === kind) {
        out.push(data[itemIdx]);
      }
    }

    out.sort(byName);

    return out;
  }

  function children(parent) {
    var out = [];

    for (var itemIdx = 0; itemIdx < data.length; itemIdx++) {
      if (data[itemIdx].parent === parent) {
        out.push(data[itemIdx]);
      }
    }

    out.sort(byName);

    return out;
  }

  function accessPill(page) {
    var kind = page.access === "internal" ? "internal" : "public";
    var label = kind === "internal" ? "Internal" : "Public";

    return '<span class="access ' + kind + '">' + label + "</span>";
  }

  function link(page, label) {
    var active = currentId() === page.id ? " active" : "";

    return '<a class="nav-link' + active + '" href="#' + encodeURIComponent(page.id) + '"><span>' + esc(label) + '</span><span class="badges">' + realms(page.realm) + "</span></a>";
  }

  function accessOf(page) {
    return page.access === "internal" ? "internal" : "public";
  }

  function splitAccess(pages) {
    var pub = [];
    var internal = [];
    var itemIdx;

    for (itemIdx = 0; itemIdx < pages.length; itemIdx++) {
      if (accessOf(pages[itemIdx]) === "internal") {
        internal.push(pages[itemIdx]);
      } else {
        pub.push(pages[itemIdx]);
      }
    }

    return { publicPages: pub, internalPages: internal };
  }

  function folder(title, pages, labelOf) {
    if (!pages.length) {
      return "";
    }

    var html = '<div class="folder"><span class="folder-name">' + esc(title) + "</span>";
    var itemIdx;

    for (itemIdx = 0; itemIdx < pages.length; itemIdx++) {
      var page = pages[itemIdx];
      html += link(page, labelOf(page));
    }

    html += "</div>";

    return html;
  }

  function group(title, pages, q) {
    var blocks = "";

    for (var itemIdx = 0; itemIdx < pages.length; itemIdx++) {
      var page = pages[itemIdx];
      var kids = children(page.name);
      var visible = [];
      var childIdx;

      for (childIdx = 0; childIdx < kids.length; childIdx++) {
        var child = kids[childIdx];

        if (child.kind === "hook") {
          continue;
        }

        if (!q || hit(page, q) || hit(child, q)) {
          visible.push(child);
        }
      }

      if (q && !hit(page, q) && visible.length === 0) {
        continue;
      }

      var split = splitAccess(visible);
      var here = byId[currentId()];
      var open = q || currentId() === page.id || (here && here.parent === page.name);
      blocks += '<div class="branch' + (open ? " open" : "") + '">' + link(page, page.name) + '<div class="kids">';
      blocks += folder("Public", split.publicPages, function (child) { return child.name; });
      blocks += folder("Internal", split.internalPages, function (child) { return child.name; });
      blocks += "</div></div>";
    }

    if (!blocks) {
      return "";
    }

    return '<section class="group"><h2>' + esc(title) + "</h2>" + blocks + "</section>";
  }

  function hookGroup(q) {
    var hooks = ofKind("hook");
    var shown = [];
    var itemIdx;

    for (itemIdx = 0; itemIdx < hooks.length; itemIdx++) {
      if (q && !hit(hooks[itemIdx], q)) {
        continue;
      }

      shown.push(hooks[itemIdx]);
    }

    var split = splitAccess(shown);
    var blocks = folder("Public", split.publicPages, function (page) { return page.parent + ":" + page.name; });
    blocks += folder("Internal", split.internalPages, function (page) { return page.parent + ":" + page.name; });

    if (!blocks) {
      return "";
    }

    return '<section class="group"><h2>Hooks</h2>' + blocks + "</section>";
  }

  function renderNav() {
    var q = query();
    nav.innerHTML = group("Classes", ofKind("class"), q) + group("Libraries", ofKind("library"), q) + hookGroup(q);
  }

  function typeHtml(ty) {
    if (!ty) {
      return "";
    }

    if (byId[ty]) {
      return '<a class="type" href="#' + encodeURIComponent(ty) + '">' + esc(ty) + "</a>";
    }

    return '<span class="type">' + esc(ty) + "</span>";
  }

  function argList(page) {
    var parts = [];
    var itemIdx;

    for (itemIdx = 0; itemIdx < page.params.length; itemIdx++) {
      var param = page.params[itemIdx];
      var piece = "";

      if (param.ty) {
        piece += typeHtml(param.ty) + " ";
      }

      piece += esc(param.name);

      if (param.optional) {
        piece += " = nil";
      }

      parts.push(piece);
    }

    return parts.join(", ");
  }

  function signature(page) {
    if (page.kind === "field") {
      var ty = page.returns[0] ? typeHtml(page.returns[0].ty) + " " : "";
      var head = page.parent ? esc(page.parent) + "." + esc(page.name) : esc(page.name);

      return ty + head;
    }

    if (page.kind === "library") {
      return esc(page.name);
    }

    if (page.kind === "class") {
      if (!page.params.length) {
        return esc(page.name);
      }

      return esc(page.name) + "(" + argList(page) + ")";
    }

    var name = page.kind === "method" || page.kind === "hook"
      ? esc(page.parent) + ":" + esc(page.name)
      : (page.parent ? esc(page.parent) + "." : "") + esc(page.name);

    return name + "(" + argList(page) + ")";
  }

  function titleOf(page) {
    if (page.kind === "method" || page.kind === "hook") {
      return page.parent + ":" + page.name;
    }

    if ((page.kind === "function" || page.kind === "field") && page.parent) {
      return page.parent + "." + page.name;
    }

    return page.name;
  }

  function realms(realm) {
    if (realm === "shared") {
      return pill("client") + pill("server");
    }

    return pill(realm);
  }

  function pill(realm) {
    var label = realm.charAt(0).toUpperCase() + realm.slice(1);

    return '<span class="realm ' + realm + '" title="' + label + '">' + label + "</span>";
  }

  function crumbs(page) {
    var html = '<a href="#">Wiki</a>';

    if (page.parent && byId[page.parent]) {
      html += ' <span>/</span> <a href="#' + encodeURIComponent(page.parent) + '">' + esc(page.parent) + "</a>";
    } else if (page.kind === "class" || page.kind === "library") {
      html += " <span>/</span> <span>" + esc(page.kind) + "</span>";
    }

    if (page.kind !== "class" && page.kind !== "library") {
      html += " <span>/</span> <span>" + esc(page.name) + "</span>";
    }

    return html;
  }

  function paramTable(page) {
    if (!page.params.length) {
      return "";
    }

    var heading = page.kind === "class" ? "Constructor" : "Arguments";
    var html = "<h3>" + heading + "</h3><table class='args'><thead><tr><th>Name</th><th>Type</th><th>Description</th></tr></thead><tbody>";
    var itemIdx;

    for (itemIdx = 0; itemIdx < page.params.length; itemIdx++) {
      var param = page.params[itemIdx];
      html += "<tr><td class='name'>" + esc(param.name);

      if (param.optional) {
        html += " <em class='opt'>optional</em>";
      }

      html += "</td><td>" + typeHtml(param.ty) + "</td><td>" + esc(param.desc) + "</td></tr>";
    }

    html += "</tbody></table>";

    return html;
  }

  function returnTable(page) {
    if (!page.returns.length) {
      return "";
    }

    var html = "<h3>Returns</h3><table class='args ret'><thead><tr><th>Type</th><th>Description</th></tr></thead><tbody>";
    var itemIdx;

    for (itemIdx = 0; itemIdx < page.returns.length; itemIdx++) {
      var item = page.returns[itemIdx];
      html += "<tr><td>" + typeHtml(item.ty) + "</td><td>" + esc(item.desc) + "</td></tr>";
    }

    html += "</tbody></table>";

    return html;
  }

  function highlight(code) {
    var re = /(--\[\[[\s\S]*?\]\]|--[^\n]*|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|\b(?:local|function|end|if|then|else|elseif|return|for|while|do|in|and|or|not|nil|true|false|break)\b|\b\d+(?:\.\d+)?\b)/g;
    var html = "";
    var last = 0;
    var match;

    while ((match = re.exec(code))) {
      html += esc(code.slice(last, match.index));
      var token = match[0];
      var kind = "num";

      if (token.slice(0, 2) === "--") {
        kind = "com";
      } else if (token.charAt(0) === '"' || token.charAt(0) === "'") {
        kind = "str";
      } else if (!/^\d/.test(token)) {
        kind = "kw";
      }

      html += "<span class='" + kind + "'>" + esc(token) + "</span>";
      last = match.index + token.length;
    }

    html += esc(code.slice(last));

    return html;
  }

  function seeAlso(page) {
    if (!page.see_also || !page.see_also.length) {
      return "";
    }

    var html = "<h3>See also</h3><ul class='seealso'>";
    var itemIdx;

    for (itemIdx = 0; itemIdx < page.see_also.length; itemIdx++) {
      var id = page.see_also[itemIdx];

      if (byId[id]) {
        html += '<li><a href="#' + encodeURIComponent(id) + '">' + esc(id) + "</a></li>";
      } else {
        html += "<li>" + esc(id) + "</li>";
      }
    }

    html += "</ul>";

    return html;
  }

  function memberSection(title, pages) {
    if (!pages.length) {
      return "";
    }

    var split = splitAccess(pages);

    if (!split.publicPages.length && !split.internalPages.length) {
      return "";
    }

    return "<h3>" + esc(title) + "</h3>" + memberFolder("Public", split.publicPages) + memberFolder("Internal", split.internalPages);
  }

  function memberFolder(title, pages) {
    if (!pages.length) {
      return "";
    }

    var html = '<h4 class="folder-title">' + esc(title) + "</h4><ul class='members'>";
    var itemIdx;

    for (itemIdx = 0; itemIdx < pages.length; itemIdx++) {
      var page = pages[itemIdx];
      html += "<li><span class='member-name'><a href='#" + encodeURIComponent(page.id) + "'>" + esc(page.name) + "</a>" + realms(page.realm) + "</span><span>" + esc(page.summary) + "</span></li>";
    }

    html += "</ul>";

    return html;
  }

  function members(page) {
    if (page.kind !== "class" && page.kind !== "library") {
      return "";
    }

    var kids = children(page.name);
    var fields = [];
    var methods = [];
    var funcs = [];
    var hooks = [];
    var itemIdx;

    for (itemIdx = 0; itemIdx < kids.length; itemIdx++) {
      var child = kids[itemIdx];

      if (child.kind === "hook") hooks.push(child);
      else if (child.kind === "field") fields.push(child);
      else if (child.kind === "method") methods.push(child);
      else funcs.push(child);
    }

    return memberSection("Fields", fields) + memberSection("Methods", methods) + memberSection("Functions", funcs) + memberSection("Hooks", hooks);
  }

  function article(page) {
    var html = "<article>";
    html += '<p class="crumbs">' + crumbs(page) + "</p>";
    html += '<div class="title-row"><div><p class="eyebrow">' + esc(page.kind) + '</p><h1>' + esc(titleOf(page)) + '</h1></div><div class="realms">' + accessPill(page) + realms(page.realm) + "</div></div>";

    if (page.deprecated) {
      var since = page.deprecated_since ? " since " + esc(page.deprecated_since) : "";
      html += '<p class="banner warn">Deprecated' + since + ". " + esc(page.deprecated) + "</p>";
    }

    if (page.unimplemented) {
      html += '<p class="banner bad">' + esc(page.unimplemented) + "</p>";
    }

    html += '<pre class="sig">' + signature(page) + "</pre>";
    html += '<p class="summary">' + esc(page.summary) + "</p>";

    if (page.since) {
      html += '<p class="since">Since ' + esc(page.since) + "</p>";
    }

    html += paramTable(page);
    html += returnTable(page);

    if (page.example) {
      html += "<h3>Example</h3><pre class='code'>" + highlight(page.example) + "</pre>";
    }

    if (page.panics) {
      html += "<h3>Errors</h3><p>" + esc(page.panics) + "</p>";
    }

    if (page.note) {
      html += "<h3>Note</h3><p>" + esc(page.note) + "</p>";
    }

    if (page.safety) {
      html += "<h3>Safety</h3><p>" + esc(page.safety) + "</p>";
    }

    html += seeAlso(page);
    html += members(page);
    html += "</article>";

    return html;
  }

  function cardRow(title, pages) {
    var html = "<h3>" + esc(title) + "</h3><div class='cards'>";
    var itemIdx;

    for (itemIdx = 0; itemIdx < pages.length; itemIdx++) {
      var page = pages[itemIdx];
      var count = children(page.name).length;
      html += '<a class="card" href="#' + encodeURIComponent(page.id) + '"><strong>' + esc(page.name) + "</strong><em>" + count + " pages</em><p>" + esc(page.summary) + "</p></a>";
    }

    html += "</div>";

    return html;
  }

  function home() {
    var html = "<article class='home'>";
    html += "<header class='hero'><p class='eyebrow'>Reference</p><h1>Scripting <em>Wiki</em></h1>";
    html += "<p class='lede'>Lua reference for this engine. Classes are values you call with a colon. Libraries are tables of functions. Hooks are callbacks you put on an entity.</p><div class='rule'></div></header>";
    html += cardRow("Classes", ofKind("class"));
    html += cardRow("Libraries", ofKind("library"));
    html += "</article>";

    return html;
  }

  function missing(id) {
    return "<article><h1>Not found</h1><p class='summary'>No page named " + esc(id) + ".</p></article>";
  }

  function renderMain() {
    var id = currentId();
    var page = byId[id];

    if (!id) {
      document.title = "Scripting Wiki";
      main.innerHTML = home();

      return;
    }

    if (!page) {
      document.title = "Not found";
      main.innerHTML = missing(id);

      return;
    }

    document.title = titleOf(page) + " — Scripting Wiki";
    main.innerHTML = article(page);
    window.scrollTo(0, 0);
  }

  function applyTheme(theme) {
    if (theme === "light") {
      document.documentElement.setAttribute("data-theme", "light");
      themeButton.textContent = "Dark";
      themeButton.setAttribute("aria-pressed", "false");

      return;
    }

    document.documentElement.removeAttribute("data-theme");
    themeButton.textContent = "Light";
    themeButton.setAttribute("aria-pressed", "true");
  }

  applyTheme(document.documentElement.getAttribute("data-theme") === "light" ? "light" : "dark");
  themeButton.addEventListener("click", function () {
    var next = document.documentElement.getAttribute("data-theme") === "light" ? "dark" : "light";

    try {
      localStorage.setItem("wiki-theme", next);
    } catch (err) {}

    applyTheme(next);
  });
  search.addEventListener("input", renderNav);
  window.addEventListener("hashchange", function () {
    renderNav();
    renderMain();
    aside.classList.remove("open");
  });
  menu.addEventListener("click", function () {
    aside.classList.toggle("open");
  });
  document.addEventListener("keydown", function (ev) {
    if (ev.key === "/" && document.activeElement !== search) {
      ev.preventDefault();
      search.focus();
    }
  });
  renderNav();
  renderMain();
})();
</script>
</body>
</html>
"####;

pub fn render(pages: &[Page]) -> String {
    let mut json = serde_json::to_string(pages).expect("wiki json");
    json = json.replace('<', "\\u003c");
    let head = HEAD.replacen("/*__WIKI_FONTS__*/", &font_css(), 1);
    let mut html = String::with_capacity(head.len() + json.len() + TAIL.len());
    html.push_str(&head);
    html.push_str(&json);
    html.push_str(TAIL);

    html
}

fn font_css() -> String {
    let mut css = String::new();
    css.push_str(&face(
        "Newsreader",
        "normal",
        400,
        include_bytes!("../fonts/Newsreader-Regular.woff2"),
    ));
    css.push_str(&face(
        "Newsreader",
        "italic",
        400,
        include_bytes!("../fonts/Newsreader-Italic.woff2"),
    ));
    css.push_str(&face(
        "Manrope",
        "normal",
        400,
        include_bytes!("../fonts/Manrope-Regular.woff2"),
    ));
    css.push_str(&face(
        "Manrope",
        "normal",
        600,
        include_bytes!("../fonts/Manrope-Semibold.woff2"),
    ));
    css.push_str(&face(
        "JetBrains Mono",
        "normal",
        400,
        include_bytes!("../fonts/JetBrainsMono-Regular.woff2"),
    ));

    css
}

fn face(family: &str, style: &str, weight: u16, bytes: &[u8]) -> String {
    format!(
        "@font-face{{font-family:\"{family}\";font-style:{style};font-weight:{weight};font-display:swap;src:url(\"data:font/woff2;base64,{}\") format(\"woff2\");}}",
        base64(bytes)
    )
}

fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    let mut idx = 0;

    while idx + 3 <= data.len() {
        let n = ((data[idx] as u32) << 16) | ((data[idx + 1] as u32) << 8) | data[idx + 2] as u32;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push(TABLE[(n & 63) as usize] as char);
        idx += 3;
    }

    let left = data.len() - idx;

    if left == 0 {
        return out;
    }

    let mut n = (data[idx] as u32) << 16;

    if left == 2 {
        n |= (data[idx + 1] as u32) << 8;
    }

    out.push(TABLE[((n >> 18) & 63) as usize] as char);
    out.push(TABLE[((n >> 12) & 63) as usize] as char);

    if left == 2 {
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push('=');
    } else {
        out.push('=');
        out.push('=');
    }

    out
}
