"use strict";

/* ------------------------------------------------------------------
   pet.js — the desktop brain
   ------------------------------------------------------------------
   cat.js knows how to *be* a cat. This file decides what the cat does:
   where it stands, when it wanders, how it reacts when the Rust side
   reports that you are watching Reels again, and what happens when you
   pick it up and let go.

   Position is driven here rather than by cat.js's own roam, because on
   the desktop the cat has a whole screen to cross, not a 300-unit
   stage. Cat.groundSpeed() is what keeps the paws from skating.

   The overlay window now covers the entire work area (not just a strip
   along the bottom) so there is room above the floor to drag the cat
   into and let gravity drop it — see fallToFloor() below.
------------------------------------------------------------------ */
(function () {
  const T = window.__TAURI__;
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  const applyMotionPreference = () => {
    if (window.gsap) { gsap.globalTimeline.timeScale(reducedMotion.matches ? 200 : 1); }
  };
  reducedMotion.addEventListener("change", applyMotionPreference);
  const invoke = T ? T.core.invoke : async () => ({});
  const listen = T ? T.event.listen : async () => {};

  // A transparent, always-on-top webview has no devtools you can open —
  // this is the only way to see what went wrong, so keep it for real
  // failure paths (never for routine per-frame chatter).
  const log = (m) => invoke("jslog", { msg: String(m) }).catch(() => {});
  window.addEventListener("error", (e) => log("ERROR " + e.message + " @" + e.lineno));

  const pet = document.getElementById("pet");
  const bubble = document.getElementById("bubble");
  const bubbleText = document.getElementById("bubbleText");
  const bubbleCount = document.getElementById("bubbleCount");

  const EDGE = 24;             // keep this far from the left/right edges
  const TOP_MARGIN = 16;       // and this far from the top
  let unit = 1.25;             // px per design unit, from settings
  let boxW = 200 * unit;
  let boxH = 150 * unit;
  let wander = true;
  let sleepy = true;
  let baseSpeed = 1;

  gsap.registerPlugin(MorphSVGPlugin);
  CatRig.mount(document.getElementById("petScene"));
  Cat.init({ state: "sit", roam: false });   // we drive position ourselves
  applyMotionPreference();

  /* ---------------------------------------------------------------
     position — both axes live in #pet's own x/y (GSAP transform),
     driven entirely from here; see pet.css for the top:0;left:0 anchor
  --------------------------------------------------------------- */
  let x = 0;
  let y = 0;

  const maxX = () => Math.max(EDGE, window.innerWidth - boxW - EDGE);
  // the rig's floor sits at y=137 of its 150-unit box, so the resting
  // position is (150-137) units short of the window's true bottom —
  // that gap is what puts the paws exactly on the taskbar line
  const floorY = () => window.innerHeight - (137 * unit);
  const minY = () => TOP_MARGIN;

  const clampX = (v) => Math.min(maxX(), Math.max(EDGE, v));
  const clampY = (v) => Math.min(floorY(), Math.max(minY(), v));

  function setXY(nx, ny) { x = nx; y = ny; gsap.set(pet, { x, y }); }
  function setX(nx) { setXY(nx, y); }

  function applyCat(cat) {
    if (!cat) { return; }
    unit = Number(cat.scale) || 1.25;
    boxW = 200 * unit;
    boxH = 150 * unit;
    baseSpeed = Number(cat.speed) || 1;
    wander = cat.wander !== false;
    sleepy = cat.sleepy !== false;
    document.documentElement.style.setProperty("--u", String(unit));
    Cat.setSpeed(baseSpeed);
    // a size change can move the floor line; snap back onto it unless
    // something is actively holding or dropping the cat right now
    if (!dragging && !falling) { setXY(clampX(x), floorY()); }
  }

  /* ---------------------------------------------------------------
     an interruptible-sequence helper

     Every behaviour is a chain of awaits. `epoch` is bumped whenever
     something more important happens (a bust, a drag, a pat), which
     makes every in-flight step resolve early and unwind the old chain
     instead of fighting the new one.
  --------------------------------------------------------------- */
  let epoch = 0;
  const stale = (mine) => mine !== epoch;
  const interrupt = () => ++epoch;

  function wait(seconds, mine) {
    return new Promise((resolve) => {
      const t = gsap.delayedCall(seconds, resolve);
      const poll = setInterval(() => {
        if (stale(mine)) { clearInterval(poll); t.kill(); resolve(); }
      }, 120);
      t.eventCallback("onComplete", () => { clearInterval(poll); resolve(); });
    });
  }

  function walkTo(targetX, mine) {
    return new Promise((resolve) => {
      const dest = clampX(targetX);
      const distance = Math.abs(dest - x);
      if (distance < 8) { resolve(); return; }

      Cat.setFacing(dest > x ? 1 : -1);
      if (Cat.getState() !== "walk" && Cat.getState() !== "angry") {
        Cat.setState("walk");
      }

      // px/sec derived from the gait itself, so the feet stay planted
      const pps = Math.max(12, Cat.groundSpeed() * unit);
      const tween = gsap.to(pet, {
        x: dest,
        duration: distance / pps,
        ease: "none",
        onUpdate() { x = gsap.getProperty(pet, "x"); },
        onComplete() { clearInterval(poll); x = dest; resolve(); }
      });
      const poll = setInterval(() => {
        if (stale(mine)) {
          clearInterval(poll); tween.kill();
          x = gsap.getProperty(pet, "x"); resolve();
        }
      }, 120);
    });
  }

  /* ---------------------------------------------------------------
     idle life
  --------------------------------------------------------------- */
  const pick = (a) => a[Math.floor(Math.random() * a.length)];
  const rand = (a, b) => a + Math.random() * (b - a);

  async function idleLoop() {
    const mine = interrupt();
    while (!stale(mine)) {
      Cat.setSpeed(baseSpeed);
      const rests = sleepy ? ["sit", "sit", "bored", "sleep", "bored"] : ["sit", "sit", "bored"];
      Cat.setState(pick(rests));
      await wait(rand(7, 18), mine);
      if (stale(mine)) { return; }

      if (wander) {
        await walkTo(rand(EDGE, maxX()), mine);
        if (stale(mine)) { return; }
        Cat.setState("sit");
        await wait(rand(1, 3), mine);
      }
    }
  }

  /* ---------------------------------------------------------------
     bubble
  --------------------------------------------------------------- */
  let bubbleTween = null;

  function showBubble(text, count, kind) {
    bubbleText.textContent = text;
    bubbleCount.textContent = count || "";
    bubbleCount.style.display = count ? "" : "none";
    bubble.className = "bubble" + (kind ? " is-" + kind : "");
    bubble.hidden = false;
    if (bubbleTween) { bubbleTween.kill(); }
    bubbleTween = gsap.fromTo(bubble,
      { opacity: 0, y: 8, scale: 0.94 },
      { opacity: 1, y: 0, scale: 1, duration: 0.28, ease: "back.out(2)", transformOrigin: "50% 100%" });
  }

  function updateBubble(count, kind) {
    bubbleCount.textContent = count || "";
    if (kind) { bubble.className = "bubble is-" + kind; }
  }

  function hideBubble() {
    if (bubbleTween) { bubbleTween.kill(); }
    bubbleTween = gsap.to(bubble, {
      opacity: 0, y: 6, duration: 0.22, ease: "power2.in",
      onComplete() { bubble.hidden = true; }
    });
  }

  /* ---------------------------------------------------------------
     the intervention
     ---------------------------------------------------------------
     Walk to the window on all fours, plant, glare through the
     countdown, then Cat.swipe() taps it closed. Every await below is
     gated on `mine` so a drag or a snooze click cleanly abandons the
     whole sequence instead of leaving it stuck.
  --------------------------------------------------------------- */
  let alerting = false;
  let countdownTimer = null;

  const SCOLDS = [
    "Shorts. Again.",
    "That's enough of that.",
    "Closing this one.",
    "You said you'd stop.",
    "Nope."
  ];

  function abandon() {
    if (countdownTimer) { clearInterval(countdownTimer); countdownTimer = null; }
    alerting = false;
    hideBubble();
  }

  async function onBust(payload) {
    if (alerting) { return; }
    alerting = true;
    const mine = interrupt();

    Cat.setSpeed(baseSpeed);
    Cat.setState("angry");                       // the fast four-beat approach

    const nagging = payload.mode !== "close";

    if (wander) {
      const targetX = typeof payload.windowCenterX === "number"
        ? payload.windowCenterX
        : (window.innerWidth - boxW) / 2 + rand(-160, 160);
      await walkTo(targetX, mine);
      if (stale(mine)) { abandon(); return; }
    }

    // arrive, plant on all fours, and hold the glare for a beat before
    // saying anything — that pause is what sells "arrived on purpose"
    Cat.setState("confront");
    await wait(0.22, mine);
    if (stale(mine)) { abandon(); return; }

    let left = nagging ? 0 : payload.countdown;
    showBubble(
      nagging ? payload.label + "?" : pick(SCOLDS),
      nagging ? "seen" : left + "s",
      nagging ? "hint" : null
    );

    if (nagging) {
      await wait(2.6, mine);
      finish(mine);
      return;
    }

    await new Promise((resolve) => {
      countdownTimer = setInterval(() => {
        left -= 1;
        if (left <= 0) { clearInterval(countdownTimer); countdownTimer = null; resolve(); }
        else { updateBubble(left + "s"); }
      }, 1000);
    });
    if (stale(mine)) { abandon(); return; }        // snoozed mid-count

    let result = { acted: false, reason: "" };
    let actCall = null;

    // the paw tap IS the close: act() fires on the strike's contact
    // frame, so the tab goes away exactly as the paw lands
    try {
      await Cat.swipe(() => {
        if (stale(mine)) { return; }               // interrupted mid-swing — don't close
        updateBubble("", "done");
        actCall = invoke("act", { actionId: payload.actionId });
      });
    } catch (err) { log("swipe failed: " + err); }

    if (stale(mine)) { abandon(); return; }

    try {
      result = await (actCall || invoke("act", { actionId: payload.actionId }));
    } catch (err) { log("act failed: " + err); }
    if (stale(mine)) { abandon(); return; }

    if (result && result.acted) {
      updateBubble("closed", "done");
      Cat.setState("pat");                        // pleased with itself
      // act() banks the XP for a successful close on the Rust side; the
      // "progress" event that follows is what plays the burst, so there
      // is nothing to award from here
    } else {
      updateBubble(result.reason || "let it go", "hint");
      Cat.setState("sit");
    }
    await wait(2.4, mine);
    finish(mine);
  }

  function finish(mine) {
    if (stale(mine)) { return; }
    alerting = false;
    hideBubble();
    Cat.setSpeed(baseSpeed);
    idleLoop();
  }

  async function cancelToSnooze() {
    if (!alerting) { return; }
    if (countdownTimer) { clearInterval(countdownTimer); countdownTimer = null; }
    alerting = false;
    interrupt();
    let mins = 5;
    try { mins = await invoke("snooze", {}); } catch (_) { }
    Cat.setSpeed(baseSpeed);
    Cat.setState("pat");
    showBubble("Fine. " + mins + " minutes.", "snoozed", "hint");
    setTimeout(() => { hideBubble(); idleLoop(); }, 2000);
  }

  /* ---------------------------------------------------------------
     pointer: click-through unless you are actually on the cat.
     Drag moves it freely in both axes; letting go above the floor
     drops it — see fallToFloor().
     ---------------------------------------------------------------
     This window (the "pet" overlay) is permanently click-through and
     never receives mouse events at all — two other approaches to a
     hover-reveal region were tried and both failed on Windows
     (set_ignore_cursor_events is all-or-nothing with no way to detect
     "the cursor just arrived"; a WM_NCHITTEST subclass answered
     per-pixel correctly but WebView2 doesn't consult it for input
     routing). The actual clicks come from a separate small "hitbox"
     window Rust keeps parked over the cat — see syncHitbox() below and
     hit.js — relayed here as "hit-down" / "hit-move" / "hit-up".
  --------------------------------------------------------------- */
  let dragging = false;
  let falling = false;
  let dragDx = 0, dragDy = 0;
  let moved = 0;

  function hitRect() {
    // the cat occupies roughly x 30..185, y 15..140 of its design box
    const u = unit;
    let left = x + 30 * u, top = y + 15 * u, right = x + 185 * u, bottom = y + 140 * u;
    if (!bubble.hidden) {
      const b = bubble.getBoundingClientRect();
      left = Math.min(left, b.left);
      top = Math.min(top, b.top);
      right = Math.max(right, b.right);
      bottom = Math.max(bottom, b.bottom);
    }
    return { left, top, right, bottom };
  }

  /* Everything crossing into Rust is in PHYSICAL pixels — work_area(),
     PhysicalPosition and PhysicalSize all are — while everything in
     here is in the webview's LOGICAL CSS pixels. On any display scaled
     past 100% the two differ by exactly this factor, so the conversion
     has to happen at the boundary in both directions. Getting this
     wrong does not fail loudly: the hitbox simply lands at a fraction
     of the cat's position (0.8x at 125%), drifting further off the
     further right the cat walks, so clicks and drags quietly never
     reach it. */
  const dpr = () => window.devicePixelRatio || 1;

  let lastSyncedKey = "";
  function syncHitbox() {
    const r = hitRect();
    const s = dpr();
    const px = {
      x: Math.round(r.left * s), y: Math.round(r.top * s),
      w: Math.round((r.right - r.left) * s), h: Math.round((r.bottom - r.top) * s)
    };
    const key = px.x + "," + px.y + "," + px.w + "," + px.h;
    if (key === lastSyncedKey) { return; }
    lastSyncedKey = key;
    invoke("set_hitbox", px).catch(() => {});
  }
  setInterval(syncHitbox, 80);

  /* Gravity, cartoon-weight: position falls off as roughly time², which
     is what "power2.in" already approximates, so a plain eased tween
     reads as a real drop without hand-rolling a physics step. Duration
     scales with drop height so a nudge off the floor is a flick and a
     drop from the top of the screen actually takes a beat. */
  const GRAVITY_PX_S2 = 4200;

  function fallToFloor(mine) {
    return new Promise((resolve) => {
      const dest = floorY();
      const dropHeight = dest - y;

      if (dropHeight <= 2) {
        // already essentially on the ground — just a tiny settle, no fall
        land(0.4);
        resolve();
        return;
      }

      falling = true;
      const duration = Math.min(1.1, Math.max(0.14, Math.sqrt((2 * dropHeight) / GRAVITY_PX_S2)));
      const tween = gsap.to(pet, {
        y: dest,
        duration,
        ease: "power2.in",
        onUpdate() { y = gsap.getProperty(pet, "y"); },
        onComplete() {
          falling = false;
          y = dest;
          land(Math.min(1, dropHeight / 240));   // harder landings squash more
          resolve();
        }
      });
      // a drag or another fall starting mid-air takes over cleanly
      const poll = setInterval(() => {
        if (stale(mine)) {
          clearInterval(poll); tween.kill(); falling = false;
          y = gsap.getProperty(pet, "y"); resolve();
        }
      }, 80);
      tween.eventCallback("onComplete", function () {
        clearInterval(poll);
        falling = false;
        y = dest;
        land(Math.min(1, dropHeight / 240));
        resolve();
      });
    });
  }

  function land(strength) {
    const squashY = 1 - 0.22 * strength;
    const squashX = 1 + 0.16 * strength;
    gsap.killTweensOf(pet, "scaleX,scaleY");
    gsap.set(pet, { transformOrigin: "50% 100%" });
    gsap.timeline()
      .to(pet, { scaleY: squashY, scaleX: squashX, duration: 0.07, ease: "power2.out" })
      .to(pet, { scaleY: 1, scaleX: 1, duration: 0.42, ease: "elastic.out(1, 0.5)" });
  }

  /* The clicks themselves arrive from the "hitbox" window, a separate
     small always-interactive window Rust keeps parked over the cat —
     see set_hitbox()/hit.js for why this is a second window rather
     than anything done in this one. Coordinates are OS-absolute screen
     pixels; treated as this window's local coordinates directly, which
     holds as long as the overlay sits at the primary work area's
     origin (true for the single/primary-monitor case this targets). */
  listen("hit-down", (e) => {
    // hit.js sends physical screen pixels; this window thinks in logical
    const s = dpr();
    const cx = e.payload.x / s, cy = e.payload.y / s;
    log("hit-down phys=" + e.payload.x + "," + e.payload.y +
        " logical=" + Math.round(cx) + "," + Math.round(cy) +
        " cat=" + Math.round(x) + "," + Math.round(y));
    dragging = true;
    moved = 0;
    dragDx = cx - x;
    dragDy = cy - y;
    interrupt();
    abandon();                                    // picking it up cancels any bust in flight
    Cat.setState("bored");                        // dangling in mid-air
  });

  listen("hit-move", (e) => {
    if (!dragging) { return; }
    const s = dpr();
    moved += 1;
    setXY(clampX(e.payload.x / s - dragDx), clampY(e.payload.y / s - dragDy));
  });

  listen("hit-up", () => {
    if (!dragging) { return; }
    dragging = false;

    if (moved < 3) {
      // a click, not a drag
      if (alerting) { cancelToSnooze(); return; }
      const mine = interrupt();
      Cat.setState("pat");
      // Rust rate-limits this, so leaning on the cat pays out at most
      // once every few seconds; the burst rides in on the event below
      invoke("award_pat", {}).catch(() => {});
      wait(3, mine).then(() => { if (!stale(mine)) { idleLoop(); } });
      return;
    }

    const mine = interrupt();
    fallToFloor(mine).then(() => {
      if (stale(mine) || alerting) { return; }
      Cat.setState("sit");
      wait(0.6, mine).then(() => { if (!stale(mine)) { idleLoop(); } });
    });
  });

  window.addEventListener("resize", () => {
    if (!dragging && !falling) { setXY(clampX(x), floorY()); }
    else { setX(clampX(x)); }
  });

  /* ---------------------------------------------------------------
     wiring
  --------------------------------------------------------------- */
  listen("bust", (e) => onBust(e.payload));

  listen("status", (e) => {
    // The cat notices a few seconds before it acts: it stops what it is
    // doing and stares. That pause is the actual warning.
    const s = e.payload;
    if (alerting || dragging) { return; }
    if (s.watching && s.remaining <= 5 && Cat.getState() !== "sit") {
      const mine = interrupt();
      Cat.setState("sit");
      wait(6, mine).then(() => { if (!stale(mine)) { idleLoop(); } });
    }
  });

  listen("config-changed", (e) => applyCat(e.payload && e.payload.cat));

  /* Every XP award broadcasts, whether it levelled or not; `gained` is
     the only thing that makes it a moment. Deliberately does NOT touch
     the state machine — the burst plays over whatever the cat is doing,
     so a level earned mid-walk doesn't stop it walking. */
  listen("progress", (e) => {
    const p = e.payload;
    if (!p) { return; }
    try {
      // the "+N XP" plays on every award; the badge only when one levelled.
      // On the award that does both, the XP reads first and the badge
      // lands under it a beat later, so they arrive as cause then effect
      // rather than both at once.
      if (p.awarded) { Cat.gainXp(p.awarded); }
      if (p.gained) {
        // the badge names the level itself, so no bubble on top of it
        gsap.delayedCall(p.awarded ? 0.45 : 0, () => Cat.levelUp(p.level));
      }
    } catch (err) { log("progress animation failed: " + err); }
  });

  // settings can drive the cat directly so you can see each pose
  listen("preview-state", (e) => {
    const mine = interrupt();
    alerting = false;
    hideBubble();
    if (e.payload === "swipe") { Cat.swipe(); }
    else { Cat.setState(e.payload); }
    wait(6, mine).then(() => { if (!stale(mine)) { idleLoop(); } });
  });

  /* ---------------------------------------------------------------
     go
  --------------------------------------------------------------- */
  setXY(Math.round((window.innerWidth - boxW) / 2), floorY());
  syncHitbox();                                   // don't wait on the first poll tick to be grabbable

  invoke("get_config", {})
    .then((cfg) => applyCat(cfg && cfg.cat))
    .catch(() => {});

  idleLoop();
})();
