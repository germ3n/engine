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
  --nav: #10151f;
  --nav-2: #1a2230;
  --ink: #e7edf5;
  --muted: #93a3b8;
  --line: #2a3548;
  --paper: #0e141d;
  --card: #171f2c;
  --accent: #7aa7ff;
  --accent-2: #8eb4ff;
  --heading: #b7c5d6;
  --soft: #c5d0de;
  --row: #243044;
  --card-2: #1c2636;
  --type: #9ec1ff;
  --shadow: rgba(0, 0, 0, 0.28);
  --sans: "Manrope", "Segoe UI", sans-serif;
  --serif: "Newsreader", Georgia, serif;
  --mono: "JetBrains Mono", ui-monospace, Menlo, Consolas, monospace;
}
html[data-theme="light"] {
  color-scheme: light;
  --nav: #172033;
  --nav-2: #1e2b42;
  --ink: #1b2433;
  --muted: #5d6d82;
  --line: #d7e0ea;
  --paper: #f3f6f9;
  --card: #ffffff;
  --accent: #2f6fed;
  --accent-2: #8eb4ff;
  --heading: #3e5168;
  --soft: #314257;
  --row: #e8eef4;
  --card-2: #f8fafc;
  --type: #2457b5;
  --shadow: rgba(23, 32, 51, 0.04);
}
* { box-sizing: border-box; }
html, body { margin: 0; padding: 0; }
body {
  font-family: var(--sans);
  color: var(--ink);
  background: var(--paper);
  font-synthesis: none;
}
button, input { font: inherit; }
#app { display: flex; align-items: flex-start; min-height: 100vh; }
aside {
  width: 292px;
  flex: none;
  position: sticky;
  top: 0;
  height: 100vh;
  overflow: auto;
  background: var(--nav);
  color: #d5deea;
  padding: 18px 14px 28px;
}
aside::-webkit-scrollbar { width: 10px; }
aside::-webkit-scrollbar-thumb { background: #31425c; border-radius: 99px; }
.brand-row {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 8px;
  padding: 4px 8px 14px;
}
.brand { padding: 0; }
#theme {
  border: 1px solid #31425c;
  background: var(--nav-2);
  color: #d5deea;
  border-radius: 999px;
  padding: 6px 10px;
  font-size: 11px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  cursor: pointer;
}
#theme:hover { border-color: var(--accent-2); }
.brand strong {
  display: block;
  color: white;
  font-family: var(--serif);
  font-weight: 400;
  font-size: 22px;
  letter-spacing: 0.01em;
}
.brand span {
  display: block;
  margin-top: 2px;
  color: #8ea0b8;
  font-size: 11px;
  letter-spacing: 0.14em;
  text-transform: uppercase;
}
#search {
  width: 100%;
  border: 1px solid #31425c;
  background: var(--nav-2);
  color: white;
  border-radius: 9px;
  padding: 9px 11px;
  outline: none;
}
#search::placeholder { color: #8ea0b8; }
#search:focus { border-color: var(--accent-2); }
.group { margin-top: 18px; }
.group h2 {
  margin: 0 8px 6px;
  font-size: 11px;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  color: #8ea0b8;
  font-weight: 650;
}
.nav-link {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  color: #d5deea;
  text-decoration: none;
  border-radius: 8px;
  padding: 6px 10px;
  font-size: 14px;
}
.nav-link:hover { background: rgba(255, 255, 255, 0.06); }
.nav-link.active {
  background: rgba(47, 111, 237, 0.28);
  color: white;
  box-shadow: inset 2px 0 0 var(--accent-2);
}
.kids { margin: 0 0 6px 10px; }
.kids .nav-link { font-size: 13px; color: #b7c5d6; }
.folder-name {
  display: block;
  margin: 8px 10px 2px;
  font-size: 10px;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  color: #8ea0b8;
  font-weight: 700;
}
h4.folder-title {
  margin: 14px 0 4px;
  font-size: 12px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--muted);
}
main {
  flex: 1;
  min-width: 0;
  padding: 28px 32px 72px;
}
article {
  max-width: 860px;
  background: var(--card);
  border: 1px solid var(--line);
  border-radius: 16px;
  padding: 26px 30px 34px;
  box-shadow: 0 10px 30px var(--shadow);
}
.crumbs { margin: 0 0 8px; color: var(--muted); font-size: 13px; }
.crumbs a { color: var(--accent); text-decoration: none; }
.crumbs a:hover { text-decoration: underline; }
.title-row {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
}
h1 { margin: 0; font-family: var(--serif); font-weight: 400; font-size: 34px; letter-spacing: -0.02em; }
.realms { display: flex; gap: 6px; flex: none; padding-top: 6px; }
.realm {
  font-size: 11px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  font-weight: 700;
  border-radius: 999px;
  padding: 4px 8px;
}
.realm.client { background: #e7f0ff; color: #1d5fd0; }
.realm.server { background: #fff1dc; color: #a15c00; }
.realm.menu { background: #f3e8ff; color: #6b3fa0; }
.lang {
  font-size: 11px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  font-weight: 700;
  border-radius: 999px;
  padding: 4px 8px;
  flex: none;
}
.lang.rust { background: #fde8e4; color: #b93824; }
.lang.lua { background: #e5f6ee; color: #1c7a4d; }
.nav-link .lang, .nav-link .realm, .nav-link .access, .member-name .realm { font-size: 9px; padding: 2px 5px; }
.access {
  font-size: 11px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  font-weight: 700;
  border-radius: 999px;
  padding: 4px 8px;
  flex: none;
}
.access.public { background: #eef1f5; color: #3d4d60; }
.access.internal { background: #f3e8f8; color: #6d3484; }
.badges { display: flex; gap: 4px; align-items: center; flex: none; }
pre.sig, pre.code {
  font-family: var(--mono);
  overflow-x: auto;
}
pre.sig {
  margin: 16px 0 0;
  padding: 14px 16px;
  background: #132033;
  color: #e7eefc;
  border-radius: 10px;
  font-size: 14px;
  line-height: 1.55;
}
pre.sig .type, pre.sig a.type { color: #9ec1ff; text-decoration: none; }
pre.sig a.type:hover { text-decoration: underline; }
.summary { font-size: 16.5px; line-height: 1.55; margin: 18px 0 0; }
.since { color: var(--muted); font-size: 13px; }
h3 {
  margin: 26px 0 10px;
  font-size: 13px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--heading);
}
table.args { width: 100%; border-collapse: collapse; }
table.args th {
  text-align: left;
  font-size: 12px;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--muted);
  font-weight: 650;
  padding: 0 12px 8px 0;
  border-bottom: 1px solid var(--line);
}
table.args td {
  padding: 10px 12px 10px 0;
  vertical-align: top;
  border-bottom: 1px solid var(--row);
  line-height: 1.45;
}
table.args td.name { font-family: var(--mono); font-weight: 400; white-space: nowrap; }
em.opt {
  font-style: normal;
  font-size: 10px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--muted);
  margin-left: 6px;
}
a.type { color: var(--accent); text-decoration: none; font-family: var(--mono); }
a.type:hover { text-decoration: underline; }
span.type { font-family: var(--mono); color: var(--type); }
pre.code {
  margin: 0;
  padding: 14px 16px;
  background: #0e1626;
  color: #e6edf3;
  border-radius: 10px;
  line-height: 1.55;
  font-size: 13.5px;
}
pre.code .kw { color: #8cb4ff; }
pre.code .str { color: #9ddeaf; }
pre.code .num { color: #ffcc80; }
pre.code .com { color: #8b9bb4; }
.banner { padding: 10px 12px; border-radius: 8px; margin: 14px 0 0; }
.banner.warn { background: #fff6e5; color: #8a5a00; }
.banner.bad { background: #fdecec; color: #9b2c2c; }
ul.members, ul.seealso { list-style: none; padding: 0; margin: 0; }
ul.members li {
  display: grid;
  grid-template-columns: minmax(180px, 260px) 1fr;
  gap: 12px;
  padding: 9px 0;
  border-bottom: 1px solid var(--row);
  align-items: center;
}
.member-name { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
ul.members a { font-family: var(--mono); color: var(--accent); text-decoration: none; }
ul.members span { color: var(--soft); }
ul.seealso li { padding: 4px 0; }
ul.seealso a { color: var(--accent); text-decoration: none; font-family: var(--mono); }
.cards {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 12px;
}
.card {
  display: block;
  text-decoration: none;
  color: inherit;
  background: var(--card-2);
  border: 1px solid var(--line);
  border-radius: 12px;
  padding: 14px 16px 16px;
}
.card:hover { border-color: #8eb0ef; }
.card strong { display: block; font-family: var(--serif); font-weight: 400; font-size: 20px; }
.card em {
  display: block;
  margin-top: 2px;
  color: var(--muted);
  font-style: normal;
  font-size: 12px;
}
.card p { margin: 8px 0 0; color: var(--soft); font-size: 14px; line-height: 1.45; }
#menu {
  display: none;
  position: fixed;
  top: 12px;
  right: 12px;
  z-index: 3;
  border: 0;
  background: var(--nav);
  color: white;
  border-radius: 999px;
  padding: 8px 12px;
}
@media (max-width: 860px) {
  #menu { display: block; }
  aside {
    position: fixed;
    z-index: 2;
    transform: translateX(-105%);
    transition: transform 0.16s ease;
    box-shadow: 8px 0 24px rgba(0, 0, 0, 0.2);
  }
  aside.open { transform: none; }
  main { padding: 64px 16px 48px; }
  article { padding: 20px 16px 28px; }
  ul.members li { grid-template-columns: 1fr; gap: 4px; }
  h1 { font-size: 24px; }
}
</style>
</head>
<body>
<div id="app">
<aside id="sidebar">
  <div class="brand-row"><div class="brand"><strong>Wiki</strong><span>Scripting API</span></div><button id="theme" type="button" aria-pressed="true">Light</button></div>
  <input id="search" type="search" placeholder="Search" autocomplete="off" aria-label="Search">
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
      blocks += '<div class="branch">' + link(page, page.name) + '<div class="kids">';
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

    return '<span class="realm ' + realm + '">' + label + "</span>";
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

    var html = "<h3>Returns</h3><table class='args'><thead><tr><th>Type</th><th>Description</th></tr></thead><tbody>";
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
    html += '<div class="title-row"><h1>' + esc(titleOf(page)) + '</h1><div class="realms">' + accessPill(page) + realms(page.realm) + "</div></div>";

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
    html += "<h1>Scripting Wiki</h1>";
    html += "<p class='summary'>Lua reference for this engine. Classes are values you call with a colon. Libraries are tables of functions. Hooks are callbacks you put on an entity.</p>";
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
