// AnvilCAD website: plays recordings while they are on screen, switches the hero
// recording between chapters, types out the agent session, cycles the B-rep figure and
// copies the build command. Everything on the page still works without it.
(function () {
  "use strict";
  document.documentElement.classList.add("js");
  var reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  // Recordings: play while visible, pause when scrolled away. With reduced motion they
  // stay on their poster until the play button is pressed.
  var videos = Array.prototype.slice.call(document.querySelectorAll("video"));
  videos.forEach(function (v) {
    var box = v.parentElement;
    var btn = document.createElement("button");
    btn.type = "button";
    btn.className = "play-toggle";
    box.appendChild(btn);
    function sync() {
      var paused = v.paused;
      btn.setAttribute("aria-label", paused ? "Play recording" : "Pause recording");
      btn.innerHTML = paused
        ? '<svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M4 2.5v11l9-5.5z" fill="currentColor"/></svg>'
        : '<svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M4 2.5h3v11H4zM9 2.5h3v11H9z" fill="currentColor"/></svg>';
      box.classList.toggle("paused", paused);
    }
    btn.addEventListener("click", function () {
      v.dataset.user = v.paused ? "play" : "pause";
      if (v.paused) { v.play().catch(function () {}); } else { v.pause(); }
    });
    v.addEventListener("play", sync);
    v.addEventListener("pause", sync);
    sync();
  });

  if ("IntersectionObserver" in window) {
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        var v = e.target;
        if (e.isIntersecting) {
          if (v.preload === "none") { v.preload = "auto"; }
          if ((!reduced && v.dataset.user !== "pause") || v.dataset.user === "play") {
            v.play().catch(function () {});
          }
        } else if (!v.paused) {
          v.pause();
        }
      });
    }, { threshold: 0.35 });
    videos.forEach(function (v) { io.observe(v); });
  }

  // Hero chapters.
  var hero = document.getElementById("hero-video");
  var caption = document.getElementById("hero-caption");
  var tabs = Array.prototype.slice.call(document.querySelectorAll(".steps [role=tab]"));
  tabs.forEach(function (tab) {
    tab.addEventListener("click", function () {
      tabs.forEach(function (t) { t.setAttribute("aria-selected", String(t === tab)); });
      var src = tab.dataset.src;
      var at = parseFloat(tab.dataset.at) || 0;
      caption.textContent = tab.dataset.caption + " · recorded with anvilcad-agent";
      function seek() {
        hero.currentTime = at;
        if (!reduced || hero.dataset.user === "play") { hero.play().catch(function () {}); }
      }
      if (hero.getAttribute("src") !== src) {
        hero.setAttribute("poster", tab.dataset.poster);
        hero.setAttribute("src", src);
        hero.addEventListener("loadedmetadata", seek, { once: true });
        hero.load();
      } else {
        seek();
      }
    });
  });

  // Agent session: reveal one line at a time the first time it scrolls into view.
  var term = document.getElementById("agent-term");
  if (term && !reduced && "IntersectionObserver" in window) {
    var lines = term.querySelectorAll(".t-line");
    term.classList.add("typing");
    var tio = new IntersectionObserver(function (entries) {
      if (!entries[0].isIntersecting) { return; }
      tio.disconnect();
      var i = 0;
      (function next() {
        if (i >= lines.length) { return; }
        lines[i].classList.add("shown");
        var out = lines[i].classList.contains("t-out");
        i += 1;
        setTimeout(next, out ? 140 : 380);
      })();
    }, { threshold: 0.4 });
    tio.observe(term);
  }

  // B-rep figure: highlight a face, then an edge, then a vertex.
  var brep = document.querySelector(".brep");
  if (brep) {
    var order = ["face", "edge", "vert"];
    var k = 0;
    brep.dataset.show = order[0];
    if (!reduced) {
      setInterval(function () {
        k = (k + 1) % order.length;
        brep.dataset.show = order[k];
      }, 2200);
    }
  }

  // Copy buttons.
  Array.prototype.forEach.call(document.querySelectorAll(".copy-btn"), function (b) {
    b.addEventListener("click", function () {
      var text = document.getElementById(b.dataset.copy).textContent;
      var done = function () {
        b.textContent = "Copied";
        setTimeout(function () { b.textContent = "Copy"; }, 1600);
      };
      if (navigator.clipboard) { navigator.clipboard.writeText(text).then(done, function () {}); }
    });
  });

  // Mobile menu.
  var menu = document.querySelector(".menu-btn");
  var nav = document.getElementById("nav");
  if (menu && nav) {
    menu.addEventListener("click", function () {
      var open = nav.classList.toggle("open");
      menu.setAttribute("aria-expanded", String(open));
    });
    nav.addEventListener("click", function (e) {
      if (e.target.tagName === "A") {
        nav.classList.remove("open");
        menu.setAttribute("aria-expanded", "false");
      }
    });
  }
})();
