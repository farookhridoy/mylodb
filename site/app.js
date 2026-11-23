// Three small jobs, each optional: the review-dialog demo in the hero, the theme preview, and the real version and download links.
// The page reads fine without any of it: the buttons already point at the releases page.

const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

// ---- the hero demo: type a DELETE, press Run, and the review dialog appears ----
(() => {
  const typed = document.getElementById("demo-typed");
  const caret = document.getElementById("demo-caret");
  const run = document.getElementById("demo-run");
  const review = document.getElementById("demo-review");
  const input = document.getElementById("demo-input");
  const go = document.getElementById("demo-go");
  const cancel = document.getElementById("demo-cancel");
  const result = document.getElementById("demo-result");
  if (!typed || !review) return;

  const SQL = "DELETE FROM orders;";
  const paint = (n) => { typed.innerHTML = SQL.slice(0, n).replace(/^DELETE/, '<span class="kw">DELETE</span>').replace(/ FROM/, ' <span class="kw">FROM</span>'); };
  const showReview = () => { review.hidden = false; };
  const reset = () => { result.hidden = true; input.value = ""; go.disabled = true; input.disabled = false; cancel.disabled = false; };

  input.addEventListener("input", () => { go.disabled = input.value.trim().toLowerCase() !== "production"; });
  go.addEventListener("click", () => {
    input.disabled = true; go.disabled = true; cancel.disabled = true;
    result.innerHTML = 'Demo only. In MyLoDB this is where the statement would run.<button type="button">Try again</button>';
    result.hidden = false;
  });
  cancel.addEventListener("click", () => {
    input.disabled = true; go.disabled = true; cancel.disabled = true;
    result.innerHTML = 'Cancelled. Nothing was sent to the server.<button type="button">Try again</button>';
    result.hidden = false;
  });
  result.addEventListener("click", (e) => { if (e.target.tagName === "BUTTON") { reset(); input.focus(); } });

  if (reduceMotion) { paint(SQL.length); caret.hidden = true; showReview(); return; }
  let i = 0;
  const step = () => {
    paint(++i);
    if (i < SQL.length) return setTimeout(step, 70 + Math.random() * 50);
    caret.hidden = true;
    setTimeout(() => { run.classList.add("press"); setTimeout(() => { run.classList.remove("press"); showReview(); }, 260); }, 420);
  };
  setTimeout(step, 900);
})();

// ---- the theme preview: the nine themes of the app ----
(() => {
  const box = document.getElementById("theme-chips");
  const mock = document.getElementById("theme-mock");
  if (!box || !mock) return;
  const THEMES = [
    ["monokai", "Monokai", "#272822"], ["github-dark", "GitHub Dark", "#0d1117"], ["github-light", "GitHub Light", "#ffffff"],
    ["dracula", "Dracula", "#282a36"], ["one-dark", "One Dark", "#282c34"], ["nord", "Nord", "#2e3440"],
    ["solarized-dark", "Solarized Dark", "#002b36"], ["solarized-light", "Solarized Light", "#fdf6e3"], ["tokyo-night", "Tokyo Night", "#1a1b26"],
  ];
  const set = (id) => {
    mock.dataset.t = id;
    box.querySelectorAll(".chip").forEach((b) => b.setAttribute("aria-checked", String(b.dataset.id === id)));
  };
  THEMES.forEach(([id, label, bg]) => {
    const b = document.createElement("button");
    b.type = "button"; b.className = "chip"; b.dataset.id = id; b.setAttribute("role", "radio");
    b.innerHTML = `<i style="background:${bg}"></i>${label}`;
    b.addEventListener("click", () => set(id));
    box.appendChild(b);
  });
  set("monokai");
})();

// ---- the latest release: version, direct download links, sizes ----
(async () => {
  try {
    const r = await fetch("https://api.github.com/repos/farookhridoy/mylodb/releases/latest", { headers: { Accept: "application/vnd.github+json" } });
    if (!r.ok) return;
    const rel = await r.json();
    const version = String(rel.tag_name || "").replace(/^v/, "");
    if (!version) return;
    document.querySelectorAll("[data-version]").forEach((el) => { el.textContent = version; });
    const dmgs = (rel.assets || []).filter((a) => /\.dmg$/.test(a.name));
    const pick = { aarch64: dmgs.find((a) => /aarch64/.test(a.name)), x64: dmgs.find((a) => /x64|x86_64/.test(a.name)) };
    const mb = (a) => `${(a.size / 1048576).toFixed(1)} MB`;
    for (const [arch, ids] of [["aarch64", ["dl-arm", "dl-arm-2"]], ["x64", ["dl-intel", "dl-intel-2"]]]) {
      const a = pick[arch];
      if (!a) continue;
      ids.forEach((id) => { const el = document.getElementById(id); if (el) el.href = a.browser_download_url; });
      document.querySelectorAll(`[data-file="${arch}"]`).forEach((el) => { el.textContent = `${a.name} (${mb(a)})`; });
    }
    const relLink = document.getElementById("dl-rel");
    if (relLink && rel.html_url) relLink.href = rel.html_url;
    const date = rel.published_at ? new Date(rel.published_at).toLocaleDateString("en", { year: "numeric", month: "long", day: "numeric" }) : "";
    const detail = document.getElementById("dl-detail");
    if (detail && date) detail.innerHTML = `Released ${date}. Each file has a SHA-256 checksum next to it on the <a href="${rel.html_url}" id="dl-rel">release page</a>.`;
  } catch { /* offline or rate limited: keep the static links */ }
})();
