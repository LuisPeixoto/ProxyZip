(function () {
  var REPO = "LuisPeixoto/ProxyZip";
  var ua = navigator.userAgent || "";
  var os = /Windows/i.test(ua) ? "win" : /Mac/i.test(ua) && !/iPhone|iPad/i.test(ua) ? "mac" : /Linux|X11/i.test(ua) && !/Android/i.test(ua) ? "linux" : "mac";
  var label = { mac: "Download for macOS", win: "Download for Windows", linux: "Download for Linux" };

  var cta = document.getElementById("cta-main");
  if (cta) cta.textContent = label[os];
  var mine = document.querySelector('.os[data-os="' + os + '"]');
  if (mine) mine.classList.add("me");

  document.querySelectorAll("[data-copy]").forEach(function (b) {
    b.addEventListener("click", function () {
      var done = function () { var t = b.textContent; b.textContent = "copied"; setTimeout(function () { b.textContent = t; }, 1400); };
      if (navigator.clipboard) navigator.clipboard.writeText(b.getAttribute("data-copy")).then(done, function () {});
    });
  });

  fetch("https://api.github.com/repos/" + REPO + "/releases/latest", { headers: { Accept: "application/vnd.github+json" } })
    .then(function (r) { return r.ok ? r.json() : Promise.reject(); })
    .then(function (rel) {
      var v = String(rel.tag_name || "").replace(/^v/, "");
      if (!v) return;
      ["ver", "ver2"].forEach(function (id) { var e = document.getElementById(id); if (e) e.textContent = (id === "ver" ? "v" : "") + v; });
      var assets = rel.assets || [];
      document.querySelectorAll("a.file").forEach(function (a) {
        var pat = a.getAttribute("data-pat");
        var hit = assets.filter(function (x) { return x.name.indexOf(pat) !== -1 && !/\.sig$/.test(x.name) && !/\.app\.tar/.test(x.name); })[0];
        if (hit) {
          a.href = hit.browser_download_url;
          a.firstChild.nodeValue = hit.name;
        }
      });
      var main = document.querySelector('.os[data-os="' + os + '"] a.file');
      if (cta && main && main.href.indexOf("/download/") !== -1) cta.href = main.href;
    })
    .catch(function () {});

  var els = document.querySelectorAll(".reveal");
  if (!("IntersectionObserver" in window)) { els.forEach(function (e) { e.classList.add("in"); }); return; }
  var io = new IntersectionObserver(function (es) {
    es.forEach(function (e) { if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); } });
  }, { rootMargin: "0px 0px -8% 0px" });
  els.forEach(function (e) { io.observe(e); });
})();
