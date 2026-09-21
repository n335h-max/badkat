"use strict";

/* ------------------------------------------------------------------
   settings.js — every knob, plus a live view of what the cat sees
   ------------------------------------------------------------------
   The live view is not decoration. Because a browser hides the page
   behind its own <title>, "why didn't it fire?" is the most common
   question this app can raise, and the only honest answer is to show
   the exact text the patterns are matched against.
------------------------------------------------------------------ */
(function () {
  const T = window.__TAURI__;
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  function applyMotionPreference() {
    if (window.gsap) { gsap.globalTimeline.timeScale(reducedMotion.matches ? 200 : 1); }
  }
  reducedMotion.addEventListener("change", applyMotionPreference);

  /* Outside Tauri (opening settings.html straight in a browser) there is
     no backend. A mock keeps the whole UI developable and inspectable
     without rebuilding the Rust side for every style tweak. */
  const mock = (() => {
    let cfg = {
      enabled: true, mode: "close", countdownSeconds: 3, snoozeMinutes: 5,
      pollMs: 1000, urlPollMs: 1500,
      dailyAllowance: { enabled: false, minutes: 30 },
      schedule: { enabled: false, weekdays: ["mon", "tue", "wed", "thu", "fri", "sat", "sun"], start: "09:00", end: "17:00" },
      never: ["zoom meeting", "microsoft teams", "google meet"],
      cat: { scale: 1.25, speed: 1, wander: true, sleepy: true },
      rules: [
        { id: "youtube-shorts", label: "YouTube Shorts", any: ["youtube.com/shorts"], all: [], grace: 6, action: "tab", enabled: true },
        { id: "instagram-reels", label: "Instagram Reels", any: ["instagram.com/reel"], all: [], grace: 6, action: "tab", enabled: true },
        { id: "tiktok", label: "TikTok", any: ["tiktok.com"], all: [], grace: 6, action: "tab", enabled: true },
        { id: "streaming", label: "Streaming", any: ["netflix.com", "crunchyroll", "hotstar"], all: [], grace: 20, action: "tab", enabled: true },
        { id: "youtube-watch", label: "YouTube", any: ["youtube.com/watch", "- youtube"], all: [], grace: 240, action: "tab", enabled: true }
      ]
    };
    return async (cmd, args) => {
      if (cmd === "get_config") { return cfg; }
      if (cmd === "save_config") { cfg = args.cfg; return { saved: true, issues: [] }; }
      if (cmd === "reset_rules") { return cfg.rules; }
      if (cmd === "check_update") {
        return { available: true, current: "0.1.0", version: "0.2.0",
                 notes: "Sample notes shown when running outside Tauri.", date: "", error: "" };
      }
      if (cmd === "get_progress") {
        return { level: 4, xp: 42, needed: 105, totalXp: 615, closes: 21, pats: 18, gained: 0, awarded: 0 };
      }
      if (cmd === "snooze") { return cfg.snoozeMinutes; }
      if (cmd === "award_pat") {
        return { level: 4, xp: 44, needed: 105, totalXp: 617, closes: 21, pats: 19, gained: 0, awarded: 2 };
      }
      if (cmd === "status") {
        return {
          enabled: true, mode: "close", snoozing: false, snoozeSecondsLeft: 0,
          foreground: { title: "(8) Instagram - Google Chrome", url: "instagram.com/reels/Dcq3rV9B6gh/", proc: "chrome", hwnd: 1, pid: 1 },
          haystack: "instagram.com/reels/dcq3rv9b6gh/ | (8) instagram - google chrome | chrome",
          matched: "Instagram Reels", remaining: 3.4, notes: [],
          usage: { state: "available", usedSeconds: 600, allowanceSeconds: 1800, remainingSeconds: 1200, localDate: "2026-09-20" },
          trail: [
            { at: "+0:12", event: "spotted", rule: "instagram-reels", detail: "(8) Instagram - Google Chrome" },
            { at: "+0:18", event: "caught", rule: "instagram-reels", detail: "(8) Instagram - Google Chrome" },
            { at: "+0:21", event: "closed", rule: "", detail: "tab" }
          ]
        };
      }
      return null;
    };
  })();

  const invoke = T ? T.core.invoke : mock;

  const $ = (id) => document.getElementById(id);
  const el = {
    enabled: $("enabled"),
    modeClose: $("modeClose"),
    modeNag: $("modeNag"),
    countdown: $("countdown"),
    snoozeMinutes: $("snoozeMinutes"),
    pollMs: $("pollMs"),
    urlPollMs: $("urlPollMs"),
    dailyEnabled: $("dailyEnabled"),
    dailyMinutes: $("dailyMinutes"),
    scheduleEnabled: $("scheduleEnabled"),
    scheduleStart: $("scheduleStart"),
    scheduleEnd: $("scheduleEnd"),
    usageStatus: $("usageStatus"),
    never: $("neverList"),
    ruleRows: $("ruleRows"),
    catScale: $("catScale"),
    catSpeed: $("catSpeed"),
    catScaleOut: $("catScaleOut"),
    catSpeedOut: $("catSpeedOut"),
    catWander: $("catWander"),
    catSleepy: $("catSleepy"),
    savedPill: $("savedPill"),
    brandState: $("brandState")
  };

  let cfg = null;
  let loading = true;

  /* ---------------- nav ---------------- */
  document.querySelectorAll(".nav").forEach((btn) => {
    btn.addEventListener("click", () => {
      document.querySelectorAll(".nav").forEach((b) => b.removeAttribute("aria-current"));
      btn.setAttribute("aria-current", "page");
      document.querySelectorAll(".panel").forEach((p) => {
        const active = p.dataset.panel === btn.dataset.panel;
        p.classList.toggle("is-on", active);
        p.hidden = !active;
      });
      const heading = document.querySelector("#" + btn.getAttribute("aria-controls") + " h2");
      if (heading) { heading.setAttribute("tabindex", "-1"); heading.focus(); }
    });
  });
  document.querySelectorAll(".panel:not(.is-on)").forEach((panel) => { panel.hidden = true; });

  /* ---------------- a live cat in the sidebar ---------------- */
  try {
    CatRig.mount(document.getElementById("brandMount"));
    gsap.registerPlugin(MorphSVGPlugin);
    Cat.init({ state: "sit", roam: false });
    applyMotionPreference();
  } catch (_) { /* the rig is decoration here; never block settings on it */ }

  /* ---------------- saving ---------------- */
  let saveTimer = null;
  function markSaved(text = "saved", error = false) {
    el.savedPill.textContent = text;
    el.savedPill.setAttribute("role", error ? "alert" : "status");
    el.savedPill.setAttribute("aria-live", error ? "assertive" : "polite");
    el.savedPill.hidden = false;
    clearTimeout(markSaved.t);
    markSaved.t = setTimeout(() => { el.savedPill.hidden = true; }, 1400);
  }

  function showValidationIssues(issues) {
    document.querySelectorAll('[aria-invalid="true"]').forEach((node) => {
      node.removeAttribute("aria-invalid");
      node.classList.remove("invalid");
      node.removeAttribute("aria-errormessage");
    });
    document.querySelectorAll(".field-error").forEach((node) => node.remove());

    const staticFields = {
      countdownSeconds: el.countdown,
      snoozeMinutes: el.snoozeMinutes,
      pollMs: el.pollMs,
      urlPollMs: el.urlPollMs,
      "cat.scale": el.catScale,
      "cat.speed": el.catSpeed,
      "dailyAllowance.minutes": el.dailyMinutes,
      "schedule.weekdays": document.querySelector('[name="scheduleDay"]'),
      "schedule.start": el.scheduleStart,
      "schedule.end": el.scheduleEnd
    };
    let first = null;
    (issues || []).forEach((item, issueIndex) => {
      let control = staticFields[item.path];
      const match = /^rules\[(\d+)\]\.(id|label|all|any|grace|action)/.exec(item.path);
      if (match) {
        const row = el.ruleRows.children[Number(match[1])];
        control = row && row.querySelector('[data-field="' + (match[2] === "id" ? "label" : match[2]) + '"]');
      }
      if (!control) { return; }
      const error = document.createElement("span");
      error.id = "validation-error-" + issueIndex;
      error.className = "field-error";
      error.textContent = item.message;
      control.setAttribute("aria-invalid", "true");
      control.setAttribute("aria-errormessage", error.id);
      control.classList.add("invalid");
      control.insertAdjacentElement("afterend", error);
      if (!first) { first = control; }
    });
    if (first) { first.focus(); }
  }

  function scheduleSave() {
    if (loading) { return; }
    clearTimeout(saveTimer);
    saveTimer = setTimeout(async () => {
      collect();
      try {
        const result = await invoke("save_config", { cfg });
        if (result && result.saved === false) {
          showValidationIssues(result.issues);
          const message = (result.issues || []).map((item) => item.message).join("; ");
          markSaved(message || "settings are invalid", true);
          return;
        }
        showValidationIssues([]);
        markSaved();
        paintBrand();
      } catch (err) {
        console.error("save failed", err);
      }
    }, 350);
  }

  /* ---------------- form <-> config ---------------- */
  function paint() {
    loading = true;
    el.enabled.checked = cfg.enabled;
    el.modeClose.checked = cfg.mode === "close";
    el.modeNag.checked = cfg.mode !== "close";
    el.countdown.value = cfg.countdownSeconds;
    el.snoozeMinutes.value = cfg.snoozeMinutes;
    el.pollMs.value = cfg.pollMs;
    el.urlPollMs.value = cfg.urlPollMs;
    const daily = cfg.dailyAllowance || { enabled: false, minutes: 30 };
    el.dailyEnabled.checked = daily.enabled === true;
    el.dailyMinutes.value = daily.minutes ?? 30;
    const schedule = cfg.schedule || {
      enabled: false,
      weekdays: ["mon", "tue", "wed", "thu", "fri", "sat", "sun"],
      start: "09:00",
      end: "17:00"
    };
    el.scheduleEnabled.checked = schedule.enabled === true;
    el.scheduleStart.value = schedule.start || "09:00";
    el.scheduleEnd.value = schedule.end || "17:00";
    const selectedDays = new Set(schedule.weekdays || []);
    document.querySelectorAll('[name="scheduleDay"]').forEach((input) => {
      input.checked = selectedDays.has(input.value);
    });
    el.never.value = (cfg.never || []).join("\n");

    const cat = cfg.cat || {};
    el.catScale.value = cat.scale ?? 1.25;
    el.catSpeed.value = cat.speed ?? 1;
    el.catWander.checked = cat.wander !== false;
    el.catSleepy.checked = cat.sleepy !== false;
    paintCatOut();

    renderRules();
    paintBrand();
    loading = false;
  }

  function paintCatOut() {
    el.catScaleOut.textContent = Number(el.catScale.value).toFixed(2);
    el.catSpeedOut.textContent = Number(el.catSpeed.value).toFixed(2);
  }

  function paintBrand() {
    if (!cfg.enabled) { el.brandState.textContent = "paused"; return; }
    el.brandState.textContent = cfg.mode === "close" ? "on patrol" : "watching only";
  }

  function collect() {
    cfg.enabled = el.enabled.checked;
    cfg.mode = el.modeClose.checked ? "close" : "nag";
    cfg.countdownSeconds = clampNum(el.countdown.value, 0, 30, 3);
    cfg.snoozeMinutes = clampNum(el.snoozeMinutes.value, 1, 240, 5);
    cfg.pollMs = clampNum(el.pollMs.value, 250, 10000, 1000);
    cfg.urlPollMs = clampNum(el.urlPollMs.value, 500, 10000, 1500);
    cfg.dailyAllowance = {
      enabled: el.dailyEnabled.checked,
      minutes: clampNum(el.dailyMinutes.value, 0, 1440, 30)
    };
    cfg.schedule = {
      enabled: el.scheduleEnabled.checked,
      weekdays: Array.from(document.querySelectorAll('[name="scheduleDay"]:checked')).map((input) => input.value),
      start: el.scheduleStart.value,
      end: el.scheduleEnd.value
    };
    cfg.never = el.never.value.split("\n").map((s) => s.trim()).filter(Boolean);
    cfg.cat = {
      scale: Number(el.catScale.value),
      speed: Number(el.catSpeed.value),
      wander: el.catWander.checked,
      sleepy: el.catSleepy.checked
    };
    cfg.rules = readRules();
  }

  function clampNum(v, lo, hi, fallback) {
    const n = Math.round(Number(v));
    if (!Number.isFinite(n)) { return fallback; }
    return Math.min(hi, Math.max(lo, n));
  }

  /* ---------------- rules ---------------- */
  function renderRules() {
    el.ruleRows.textContent = "";
    (cfg.rules || []).forEach((rule, i) => el.ruleRows.appendChild(ruleRow(rule, i)));
  }

  function ruleRow(rule, index) {
    const row = document.createElement("div");
    row.className = "rule" + (rule.enabled === false ? " off" : "");
    row.dataset.id = rule.id;
    row.setAttribute("role", "group");
    row.setAttribute("aria-label", "Rule " + (index + 1) + ": " + (rule.label || rule.id));

    const on = document.createElement("input");
    on.type = "checkbox";
    on.checked = rule.enabled !== false;
    on.title = "Enable this rule";
    on.setAttribute("aria-label", "Enable rule " + (rule.label || index + 1));
    on.addEventListener("change", () => {
      row.classList.toggle("off", !on.checked);
      scheduleSave();
    });

    const label = input("text", rule.label || rule.id);
    label.setAttribute("aria-label", "Rule " + (index + 1) + " name");
    label.dataset.field = "label";
    const any = input("text", (rule.any || []).join(", "));
    any.setAttribute("aria-label", "Rule " + (index + 1) + " match any patterns");
    any.dataset.field = "any";
    any.classList.add("pat");
    any.placeholder = "youtube.com/shorts";
    const all = input("text", (rule.all || []).join(", "));
    all.setAttribute("aria-label", "Rule " + (index + 1) + " match all patterns");
    all.dataset.field = "all";
    all.classList.add("pat");
    all.placeholder = "(optional)";

    const grace = input("number", rule.grace);
    grace.min = 0; grace.max = 3600;
    grace.setAttribute("aria-label", "Rule " + (index + 1) + " grace seconds");
    grace.dataset.field = "grace";

    const action = document.createElement("select");
    [["tab", "Close tab"], ["close", "Close window"]].forEach(([v, t]) => {
      const o = document.createElement("option");
      o.value = v; o.textContent = t;
      if ((rule.action || "tab") === v) { o.selected = true; }
      action.appendChild(o);
    });
    action.addEventListener("change", scheduleSave);
    action.setAttribute("aria-label", "Rule " + (index + 1) + " action");
    action.dataset.field = "action";

    const del = document.createElement("button");
    del.className = "iconbtn";
    del.textContent = "×";
    del.title = "Delete this rule";
    del.setAttribute("aria-label", "Delete rule " + (rule.label || index + 1));
    del.addEventListener("click", () => {
      collect();
      cfg.rules.splice(index, 1);
      renderRules();
      const rows = Array.from(el.ruleRows.children);
      const focusRow = rows[Math.min(index, rows.length - 1)];
      const focusTarget = focusRow && focusRow.querySelector('[data-field="label"]');
      (focusTarget || $("addRule")).focus();
      scheduleSave();
    });

    row.append(on, label, any, all, grace, action, del);
    row._read = () => ({
      id: rule.id,
      label: label.value.trim(),
      any: splitList(any.value),
      all: splitList(all.value),
      grace: clampNum(grace.value, 0, 3600, 10),
      action: action.value,
      enabled: on.checked
    });
    return row;
  }

  function input(type, value) {
    const i = document.createElement("input");
    i.type = type;
    i.value = value ?? "";
    i.addEventListener("input", scheduleSave);
    return i;
  }

  const splitList = (s) => s.split(",").map((x) => x.trim()).filter(Boolean);

  function readRules() {
    return Array.from(el.ruleRows.children)
      .map((row) => row._read());
  }

  $("addRule").addEventListener("click", () => {
    collect();
    cfg.rules.push({
      id: "custom-" + Date.now().toString(36),
      label: "New rule",
      any: [],
      all: [],
      grace: 15,
      action: "tab",
      enabled: true
    });
    renderRules();
    const added = el.ruleRows.lastElementChild;
    const name = added && added.querySelector('[data-field="label"]');
    if (name) { name.focus(); name.select(); }
  });

  $("resetRules").addEventListener("click", async () => {
    cfg.rules = await invoke("reset_rules", {});
    renderRules();
    markSaved();
  });

  /* ---------------- the cat panel ---------------- */
  [el.catScale, el.catSpeed].forEach((r) =>
    r.addEventListener("input", () => { paintCatOut(); scheduleSave(); }));

  document.querySelectorAll("#poseBtns .btn").forEach((b) => {
    b.addEventListener("click", () => {
      invoke("preview_state", { name: b.dataset.pose }).catch(() => {});
      try {
        if (b.dataset.pose === "swipe") { Cat.swipe(); }
        else { Cat.setState(b.dataset.pose); }
      } catch (_) {}
    });
  });

  $("previewBust").addEventListener("click", () => invoke("preview_bust", {}).catch(() => {}));
  $("snoozeNow").addEventListener("click", async () => {
    try {
      const minutes = await invoke("snooze", {});
      markSaved("Snoozed for " + minutes + " minutes");
    } catch (err) { markSaved("Could not snooze", true); }
  });
  $("patNow").addEventListener("click", async () => {
    try {
      await invoke("award_pat", {});
      markSaved("Cat patted");
    } catch (err) { markSaved("Could not pat the cat", true); }
  });

  let catVisible = true;
  $("toggleCat").addEventListener("click", (e) => {
    catVisible = !catVisible;
    invoke("set_cat_visible", { visible: catVisible }).catch(() => {});
    e.target.textContent = catVisible ? "Hide the cat" : "Show the cat";
  });

  /* ---------------- generic bindings ---------------- */
  [el.enabled, el.modeClose, el.modeNag, el.catWander, el.catSleepy,
   el.dailyEnabled, el.scheduleEnabled, ...document.querySelectorAll('[name="scheduleDay"]')]
    .forEach((n) => n.addEventListener("change", scheduleSave));
  [el.countdown, el.snoozeMinutes, el.pollMs, el.urlPollMs, el.never,
   el.dailyMinutes, el.scheduleStart, el.scheduleEnd]
    .forEach((n) => n.addEventListener("input", scheduleSave));

  /* ---------------- live view ---------------- */
  const live = {
    title: $("liveTitle"), url: $("liveUrl"), proc: $("liveProc"),
    rule: $("liveRule"), remaining: $("liveRemaining"),
    hay: $("liveHay"), trail: $("liveTrail"), notes: $("liveNotes")
  };

  async function pollStatus() {
    let s;
    try { s = await invoke("status", {}); } catch (_) { return; }

    const fg = s.foreground;
    live.title.textContent = fg && fg.title ? fg.title : "—";
    live.url.textContent = fg && fg.url ? fg.url : (fg ? "(no address bar — fullscreen video, or not a browser)" : "—");
    live.proc.textContent = fg && fg.proc ? fg.proc : "—";
    live.hay.textContent = s.haystack || "—";

    const usage = s.usage;
    if (!usage || usage.state === "disabled") {
      el.usageStatus.textContent = "Daily allowance is off.";
    } else if (usage.state === "outside-schedule") {
      el.usageStatus.textContent = "Outside scheduled patrol hours.";
    } else if (usage.state === "exhausted") {
      el.usageStatus.textContent = "Today's allowance is used; normal enforcement is active.";
    } else {
      el.usageStatus.textContent = Math.ceil((usage.remainingSeconds || 0) / 60) + " minutes remaining today.";
    }

    if (s.snoozing) {
      live.rule.textContent = "snoozed";
      live.remaining.textContent = Math.ceil(s.snoozeSecondsLeft / 60) + " min left";
    } else if (s.matched) {
      live.rule.textContent = s.matched;
      live.remaining.textContent = s.remaining > 0
        ? s.remaining.toFixed(1) + "s"
        : (s.mode === "close" ? "now" : "watching only");
    } else {
      live.rule.textContent = "nothing — this is fine";
      live.remaining.textContent = "—";
    }

    if (s.trail && s.trail.length) {
      live.trail.textContent = "";
      s.trail.slice().reverse().forEach((t) => {
        const d = document.createElement("div");
        d.innerHTML = "<span class='t'></span><span class='e'></span><span class='d'></span>";
        d.children[0].textContent = t.at;
        d.children[1].textContent = t.event;
        d.children[1].classList.add(t.event);
        d.children[2].textContent = t.rule ? t.rule + " · " + t.detail : t.detail;
        live.trail.appendChild(d);
      });
    }

    live.notes.textContent = "";
    (s.notes || []).forEach((n) => {
      const box = document.createElement("div");
      box.className = "warnbox";
      box.textContent = n;
      live.notes.appendChild(box);
    });
  }

  /* ---------------- affection ---------------- */
  const lvl = {
    num: $("lvlNum"), xp: $("lvlXp"), fill: $("xpFill"), note: $("lvlNote"),
    card: document.querySelector(".levelcard"),
    statLevel: $("statLevel"), statClosed: $("statClosed"),
    statPats: $("statPats"), statXp: $("statXp")
  };

  function paintProgress(p) {
    if (!p) { return; }
    const pct = p.needed > 0 ? Math.min(100, (p.xp / p.needed) * 100) : 0;
    lvl.num.textContent = p.level;
    lvl.xp.textContent = p.xp + " / " + p.needed;
    lvl.fill.style.width = pct.toFixed(1) + "%";
    lvl.note.textContent = (p.needed - p.xp) + " XP to level " + (p.level + 1);
    lvl.statLevel.textContent = p.level;
    lvl.statClosed.textContent = p.closes;
    lvl.statPats.textContent = p.pats;
    lvl.statXp.textContent = p.totalXp;

    if (p.gained) {
      // restart the flash even if it is mid-animation from a previous level
      lvl.card.classList.remove("is-up");
      void lvl.card.offsetWidth;
      lvl.card.classList.add("is-up");
    }
  }

  invoke("get_progress", {}).then(paintProgress).catch(() => {});
  if (T) { T.event.listen("progress", (e) => paintProgress(e.payload)); }

  /* ---------------- updates ---------------- */
  const upd = {
    current: $("updCurrent"), status: $("updStatus"), check: $("updCheck"),
    install: $("updInstall"), track: $("updTrack"), fill: $("updFill"),
    notes: $("updReleaseNotes")
  };
  const SITE_URL = "https://badkat.cypherion.tech";
  const REPO_URL = "https://github.com/X-DIABLO-X/badkat";

  function setUpdStatus(text, kind) {
    upd.status.textContent = text;
    upd.status.className = "updnote" + (kind ? " is-" + kind : "");
  }

  async function checkForUpdate(manual) {
    upd.check.disabled = true;
    if (manual) { setUpdStatus("Checking…"); }
    let info;
    try { info = await invoke("check_update", {}); }
    catch (err) { setUpdStatus("Could not check: " + err, "error"); upd.check.disabled = false; return; }
    upd.check.disabled = false;

    if (info.current) { upd.current.textContent = "v" + info.current; }

    if (info.error) {
      // a missing latest.json is the normal state before the first
      // release, so say something truer than "update failed"
      setUpdStatus("Could not reach the update server. " + info.error, "error");
      upd.install.hidden = true;
      return;
    }
    if (info.available) {
      setUpdStatus("Version " + info.version + " is available.", "ready");
      upd.install.hidden = false;
      upd.install.textContent = "Update to " + info.version;
      if (info.notes) { upd.notes.hidden = false; upd.notes.textContent = info.notes; }
    } else {
      setUpdStatus("You are on the latest version.", "current");
      upd.install.hidden = true;
      upd.notes.hidden = true;
    }
  }

  if (upd.check) {
    upd.check.addEventListener("click", () => checkForUpdate(true));

    upd.install.addEventListener("click", async () => {
      upd.install.disabled = true;
      upd.check.disabled = true;
      upd.track.hidden = false;
      setUpdStatus("Downloading…");
      try {
        await invoke("install_update", {});
        // on Windows the installer takes over and restarts the app, so
        // reaching here at all usually means it is about to disappear
        setUpdStatus("Installing — BadKat will restart.", "ready");
      } catch (err) {
        setUpdStatus("Update failed: " + err, "error");
        upd.install.disabled = false;
        upd.check.disabled = false;
        upd.track.hidden = true;
      }
    });

    if (T) {
      T.event.listen("update-progress", (e) => {
        const p = e.payload || {};
        if (p.installing) { setUpdStatus("Installing — BadKat will restart.", "ready"); upd.fill.style.width = "100%"; return; }
        if (typeof p.percent === "number" && p.total) {
          upd.fill.style.width = Math.min(100, p.percent).toFixed(1) + "%";
          setUpdStatus("Downloading… " + Math.round(p.percent) + "%");
        } else if (p.downloaded) {
          setUpdStatus("Downloading… " + (p.downloaded / 1048576).toFixed(1) + " MB");
        }
      });
    }

    // the backend holds the actual URLs; this only names which one
    const openExternal = (which, fallback) => {
      if (T) { invoke("open_link", { which }).catch(() => {}); }
      else { window.open(fallback, "_blank"); }
    };
    $("openSite").addEventListener("click", () => openExternal("site", SITE_URL));
    $("openRepo").addEventListener("click", () => openExternal("repo", REPO_URL));

    // a quiet check on open, so the dashboard already knows by the time
    // you go looking for it
    checkForUpdate(false);
  }

  /* ---------------- go ---------------- */
  invoke("get_config", {}).then((loaded) => {
    cfg = loaded;
    paint();
    pollStatus();
    setInterval(pollStatus, 700);
  }).catch((err) => {
    document.querySelector(".panels").innerHTML =
      "<div class='warnbox'>Could not reach the app backend: " + err + "</div>";
  });
})();
