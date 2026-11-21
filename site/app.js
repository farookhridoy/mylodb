// Shows the latest release's version and a direct download link. The page works without it: the buttons already point at the releases page.
(async () => {
  try {
    const r = await fetch("https://api.github.com/repos/farookhridoy/mylodb/releases/latest", { headers: { Accept: "application/vnd.github+json" } });
    if (!r.ok) return;
    const rel = await r.json();
    const dmg = (rel.assets || []).find((a) => /\.dmg$/.test(a.name));
    if (!dmg) return;
    const btn = document.getElementById("dl");
    btn.href = dmg.browser_download_url;
    btn.textContent = `Download MyLoDB ${rel.tag_name.replace(/^v/, "")} for Mac`;
    const mb = (dmg.size / 1048576).toFixed(1);
    const date = rel.published_at ? new Date(rel.published_at).toLocaleDateString("en", { year: "numeric", month: "short", day: "numeric" }) : "";
    document.getElementById("dl-meta").textContent = `Apple Silicon (M1 and later) · ${mb} MB · released ${date} · free · no telemetry`;
    document.getElementById("dl2").href = rel.html_url;
  } catch { /* offline or rate limited: keep the static links */ }
})();
