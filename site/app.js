// Shows the latest release's version and a direct download link. The page works without it: the buttons already point at the releases page.
(async () => {
  try {
    const r = await fetch("https://api.github.com/repos/farookhridoy/mylodb/releases/latest", { headers: { Accept: "application/vnd.github+json" } });
    if (!r.ok) return;
    const rel = await r.json();
    const dmgs = (rel.assets || []).filter((a) => /\.dmg$/.test(a.name));
    const dmg = dmgs.find((a) => /aarch64/.test(a.name)) || dmgs[0];
    const intel = dmgs.find((a) => /x64|x86_64/.test(a.name));
    if (!dmg) return;
    const btn = document.getElementById("dl");
    btn.href = dmg.browser_download_url;
    btn.textContent = `Download MyLoDB ${rel.tag_name.replace(/^v/, "")} for Apple Silicon`;
    if (intel) { const b = document.getElementById("dl-intel"); b.href = intel.browser_download_url; b.hidden = false; }
    const mb = (dmg.size / 1048576).toFixed(1);
    const date = rel.published_at ? new Date(rel.published_at).toLocaleDateString("en", { year: "numeric", month: "short", day: "numeric" }) : "";
    document.getElementById("dl-meta").textContent = `${intel ? "Apple Silicon (M1 and later) or Intel" : "Apple Silicon (M1 and later)"} · ${mb} MB · released ${date} · updates itself · free · no telemetry`;
    document.getElementById("dl2").href = rel.html_url;
  } catch { /* offline or rate limited: keep the static links */ }
})();
