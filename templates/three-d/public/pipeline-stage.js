/* <pipeline-stage> — three.js view of window.PipelineSim.
   Cyber-Physical skin: dark infinite grid, wireframe edges, neon ink/amber/red.
   Built for long background runs: shared geometry + materials, one directional
   light, no postprocessing, capped pixel ratio, paused when hidden. */
(function () {
  var C = {
    chassis: 0x1c1b19, rule: 0x383530, well: 0x2a2825,
    ink: 0x7fd1c1, signal: 0xe8a33d, fault: 0xc25b4a, paper: 0xe6e1d8, muted: 0x8a857c
  };

  function ready(cb) {
    if (window.THREE && window.PipelineSim) return cb();
    var t = setInterval(function () {
      if (window.THREE && window.PipelineSim) { clearInterval(t); cb(); }
    }, 40);
  }

  class PipelineStage extends HTMLElement {
    connectedCallback() {
      if (this._booted) return;
      this._booted = true;
      this.style.display = "block";
      this.style.position = "relative";
      this.style.width = "100%";
      this.style.height = "100%";
      ready(this.boot.bind(this));
    }

    disconnectedCallback() {
      this._dead = true;
      if (this._ro) this._ro.disconnect();
      if (this.renderer) this.renderer.dispose();
    }

    get mode() { return this.getAttribute("mode") === "detail" ? "detail" : "ambient"; }

    /** HUD insets, so the scene centres in the free area rather than behind panels. */
    get pads() {
      var n = function (v) { return parseFloat(v || "0") || 0; };
      var a = function (el, name) { return el.getAttribute(name) || el.getAttribute(name.replace("-", "")); };
      return {
        l: n(a(this, "pad-left")), r: n(a(this, "pad-right")),
        t: n(a(this, "pad-top")), b: n(a(this, "pad-bottom"))
      };
    }

    boot() {
      var T = window.THREE, sim = window.PipelineSim;
      this.T = T; this.sim = sim;

      // Software renderers, locked-down browsers, and headless VMs all reach
      // here without WebGL. The HUD panels are plain DOM and keep working, so
      // say what is missing rather than throwing and leaving a blank page.
      var renderer;
      try {
        renderer = new T.WebGLRenderer({ antialias: true, powerPreference: "low-power" });
      } catch (err) {
        this.failed = true;
        var notice = document.createElement("div");
        notice.className = "pipeline-stage-nogl";
        notice.textContent =
          "3D view unavailable: this browser reports no WebGL context. " +
          "The stage rail, item list, and hold reasons on this page are still live.";
        this.appendChild(notice);
        return;
      }
      renderer.setClearColor(C.chassis, 1);
      renderer.shadowMap.enabled = false;
      this.renderer = renderer;
      renderer.domElement.style.display = "block";
      renderer.domElement.style.cursor = "grab";
      this.appendChild(renderer.domElement);

      var scene = new T.Scene();
      scene.fog = new T.FogExp2(0x141311, 0.026);
      this.scene = scene;

      var camera = new T.PerspectiveCamera(40, 1, 0.5, 300);
      this.camera = camera;

      scene.add(new T.HemisphereLight(0x5c6b68, 0x141312, 0.55));
      var dir = new T.DirectionalLight(0xcfe6e0, 0.5);
      dir.position.set(-8, 14, 10);
      scene.add(dir);

      // Infinite dark grid, the floor of the Cyber-Physical skin.
      var grid = new T.GridHelper(200, 100, C.ink, C.well);
      grid.material.transparent = true;
      grid.material.opacity = 0.18;
      grid.position.y = -0.001;
      scene.add(grid);
      this.grid = grid;

      this.mats = {
        chassis: new T.MeshStandardMaterial({ color: C.well, roughness: 0.85, metalness: 0.25 }),
        chassisDark: new T.MeshStandardMaterial({ color: 0x201f1d, roughness: 0.9, metalness: 0.15 }),
        edge: new T.LineBasicMaterial({ color: C.ink, transparent: true, opacity: 0.5 }),
        edgeDim: new T.LineBasicMaterial({ color: C.rule, transparent: true, opacity: 0.7 }),
        glass: new T.MeshStandardMaterial({
          color: C.ink, roughness: 0.25, metalness: 0.1, transparent: true, opacity: 0.16,
          emissive: C.ink, emissiveIntensity: 0.35
        }),
        active: new T.MeshStandardMaterial({ color: 0x0f1a18, emissive: C.ink, emissiveIntensity: 1.6, roughness: 0.4 }),
        held: new T.MeshStandardMaterial({ color: 0x1c1408, emissive: C.signal, emissiveIntensity: 1.8, roughness: 0.4 }),
        fault: new T.MeshStandardMaterial({ color: 0x1a0d0a, emissive: C.fault, emissiveIntensity: 1.6, roughness: 0.5 }),
        faultWire: new T.MeshBasicMaterial({ color: C.fault, wireframe: true, transparent: true, opacity: 0.9 }),
        queued: new T.MeshStandardMaterial({ color: 0x141312, emissive: C.muted, emissiveIntensity: 0.9, roughness: 0.7 })
      };

      this.geo = {
        entity: new T.BoxGeometry(0.17, 0.17, 0.17),
        shard: new T.TetrahedronGeometry(0.2),
        warn: new T.ConeGeometry(0.17, 0.42, 4)
      };

      this.nodes = {};
      this.buildNodes();
      this.buildEdges();

      this.entities = new Map();
      this.pool = [];
      this.entityLayer = new T.Group();
      scene.add(this.entityLayer);

      this.orbit = { angle: 0.9, radius: 0, height: 0, target: new T.Vector3(), manual: 0 };
      this.frameCamera();
      this.bindInput();

      this._ro = new ResizeObserver(this.resize.bind(this));
      this._ro.observe(this);
      this.resize();

      this.hidden_ = false;
      document.addEventListener("visibilitychange", function () {
        this.hidden_ = document.hidden;
      }.bind(this));

      this.last = performance.now();
      this.loop();
    }

    resize() {
      var w = this.clientWidth || 1, h = this.clientHeight || 1;
      var cap = this.mode === "detail" ? 2 : 1.35;
      this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, cap));
      // updateStyle must stay on. With it off the canvas has no CSS size, so its
      // intrinsic size — the backing store, width x pixelRatio — becomes its layout
      // size and the scene renders devicePixelRatio-times too large on a HiDPI
      // display, pushing the far stages off screen.
      this.renderer.setSize(w, h, true);
      this.camera.aspect = w / h;
      this.camera.updateProjectionMatrix();
    }

    /* ---------- geometry ---------- */

    label(text, sub) {
      var T = this.T;
      var cv = document.createElement("canvas");
      cv.width = 512; cv.height = 128;
      var g = cv.getContext("2d");
      g.clearRect(0, 0, 512, 128);
      g.font = "500 40px ui-monospace, 'SF Mono', Menlo, monospace";
      g.fillStyle = "#e6e1d8";
      g.textAlign = "center";
      g.fillText(text, 256, 52);
      g.font = "500 24px ui-monospace, Menlo, monospace";
      g.fillStyle = "#8a857c";
      g.letterSpacing = "6px";
      g.fillText(sub.toUpperCase(), 256, 92);
      var tex = new T.CanvasTexture(cv);
      tex.anisotropy = 2;
      var sprite = new T.Sprite(new T.SpriteMaterial({ map: tex, transparent: true, depthWrite: false, opacity: 0.75 }));
      // Deliberately smaller than the node body. The stage name is orientation,
      // not the reading — the geometry and the item colours are what carry state.
      sprite.scale.set(3.2, 0.8, 1);
      return sprite;
    }

    wire(geometry, mat) {
      var T = this.T;
      return new T.LineSegments(new T.EdgesGeometry(geometry), mat || this.mats.edge);
    }

    /** Source — a rotating vortex ring where items materialise. */
    buildPortal() {
      var T = this.T, m = this.mats, parts = {};
      var g = new T.Group();
      var plinthGeo = new T.CylinderGeometry(1.5, 1.8, 0.3, 8);
      var plinth = new T.Mesh(plinthGeo, m.chassisDark);
      plinth.position.y = 0.15;
      g.add(plinth, this.wire(plinthGeo, m.edgeDim).translateY(0.15));

      var rings = new T.Group();
      rings.position.y = 1.5;
      rings.rotation.y = Math.PI / 2;
      var r1 = new T.Mesh(new T.TorusGeometry(1.25, 0.075, 6, 40), m.glass.clone());
      var r2 = new T.Mesh(new T.TorusGeometry(0.95, 0.045, 6, 32), m.active.clone());
      var r3 = new T.Mesh(new T.TorusGeometry(0.62, 0.03, 6, 24), m.active.clone());
      rings.add(r1, r2, r3);
      g.add(rings);
      parts.rings = [r1, r2, r3];

      var fins = new T.Group();
      fins.position.y = 1.5;
      var finGeo = new T.BoxGeometry(0.07, 0.46, 0.07);
      for (var i = 0; i < 8; i++) {
        var f = new T.Mesh(finGeo, m.chassis);
        var a = (i / 8) * Math.PI * 2;
        f.position.set(0, Math.sin(a) * 1.55, Math.cos(a) * 1.55);
        f.rotation.x = -a;
        fins.add(f);
      }
      g.add(fins);
      parts.fins = fins;

      var core = new T.Mesh(new T.CircleGeometry(0.9, 24), new T.MeshBasicMaterial({
        color: C.ink, transparent: true, opacity: 0.22, side: T.DoubleSide, depthWrite: false
      }));
      core.position.y = 1.5;
      core.rotation.y = Math.PI / 2;
      g.add(core);
      parts.core = core;

      return { group: g, parts: parts, kind: "portal" };
    }

    /** Transform — a crystal that refracts work as it passes through. */
    buildPrism() {
      var T = this.T, m = this.mats, parts = {};
      var g = new T.Group();
      var baseGeo = new T.CylinderGeometry(1.25, 1.45, 0.28, 6);
      var base = new T.Mesh(baseGeo, m.chassisDark);
      base.position.y = 0.14;
      g.add(base, this.wire(baseGeo, m.edgeDim).translateY(0.14));

      var finGeo = new T.BoxGeometry(0.07, 1.2, 0.34);
      parts.fins = [];
      for (var i = 0; i < 4; i++) {
        var f = new T.Mesh(finGeo, m.chassis);
        var a = (i / 4) * Math.PI * 2 + Math.PI / 4;
        f.position.set(Math.cos(a) * 1.05, 0.7, Math.sin(a) * 1.05);
        f.rotation.y = -a;
        g.add(f);
        parts.fins.push(f);
      }

      var coreGeo = new T.OctahedronGeometry(0.8);
      var core = new T.Mesh(coreGeo, m.glass.clone());
      core.position.y = 1.55;
      var coreWire = this.wire(coreGeo);
      coreWire.position.y = 1.55;
      g.add(core, coreWire);
      parts.core = core;
      parts.coreWire = coreWire;

      var shards = new T.Group();
      shards.position.y = 1.55;
      parts.shards = [];
      for (var s = 0; s < 3; s++) {
        var sh = new T.Mesh(this.geo.shard, m.active.clone());
        var sa = (s / 3) * Math.PI * 2;
        sh.position.set(Math.cos(sa) * 1.25, 0, Math.sin(sa) * 1.25);
        shards.add(sh);
        parts.shards.push(sh);
      }
      g.add(shards);
      parts.shardRig = shards;

      var ring = new T.Mesh(new T.TorusGeometry(1.5, 0.022, 5, 40), m.active.clone());
      ring.rotation.x = Math.PI / 2;
      ring.position.y = 0.42;
      g.add(ring);
      parts.ring = ring;

      return { group: g, parts: parts, kind: "prism" };
    }

    /** Sink — a well whose floor matrix lights as blocks slot into place. */
    buildArchive() {
      var T = this.T, m = this.mats, parts = {};
      var g = new T.Group();

      parts.tiles = [];
      var tileGeo = new T.BoxGeometry(0.42, 0.07, 0.42);
      var tileMat = new T.MeshStandardMaterial({ color: 0x1d1c1a, emissive: C.ink, emissiveIntensity: 0, roughness: 0.8 });
      for (var i = 0; i < 5; i++) {
        for (var j = 0; j < 5; j++) {
          var t = new T.Mesh(tileGeo, tileMat.clone());
          t.position.set((i - 2) * 0.55, 0.035, (j - 2) * 0.55);
          t.userData.d = Math.hypot(i - 2, j - 2);
          g.add(t);
          parts.tiles.push(t);
        }
      }

      parts.steps = [];
      for (var s = 0; s < 3; s++) {
        var size = 3.1 - s * 0.55;
        var stepGeo = new T.BoxGeometry(size, 0.12, size);
        var frame = this.wire(stepGeo, s === 0 ? this.mats.edgeDim : this.mats.edge);
        frame.position.y = 0.5 + s * 0.5;
        g.add(frame);
        parts.steps.push(frame);
      }

      var slabGeo = new T.BoxGeometry(0.24, 1.9, 0.24);
      for (var k = 0; k < 4; k++) {
        var slab = new T.Mesh(slabGeo, m.chassis);
        var a = (k / 4) * Math.PI * 2 + Math.PI / 4;
        slab.position.set(Math.cos(a) * 1.6, 0.95, Math.sin(a) * 1.6);
        g.add(slab);
      }

      var pool = new T.Mesh(new T.CircleGeometry(1.9, 32), new T.MeshBasicMaterial({
        color: C.ink, transparent: true, opacity: 0.06, depthWrite: false
      }));
      pool.rotation.x = -Math.PI / 2;
      pool.position.y = 0.01;
      g.add(pool);
      parts.pool = pool;

      return { group: g, parts: parts, kind: "archive" };
    }

    buildNodes() {
      var T = this.T, sim = this.sim;
      var ids = Object.keys(sim.nodes);
      var self = this;
      ids.forEach(function (id) {
        var state = sim.nodes[id];
        var built = state.kind === "source" ? self.buildPortal()
          : state.kind === "sink" ? self.buildArchive()
            : self.buildPrism();
        var pos = sim.positions[id] || { x: 0, z: 0 };
        built.group.position.set(pos.x, 0, pos.z);

        var label = self.label(state.display_name, state.kind);
        label.position.set(0, 3.5, 0);
        built.group.add(label);

        var warn = new T.Mesh(self.geo.warn, self.mats.held.clone());
        warn.position.set(0, 2.85, 0);
        warn.visible = false;
        built.group.add(warn);
        built.parts.warn = warn;

        var hit = new T.Mesh(new T.SphereGeometry(2.1, 8, 6), new T.MeshBasicMaterial({ visible: false }));
        hit.position.y = 1.4;
        hit.userData.nodeId = id;
        built.group.add(hit);
        built.hit = hit;

        var sel = new T.Mesh(new T.TorusGeometry(2.1, 0.02, 4, 48), self.mats.active.clone());
        sel.rotation.x = Math.PI / 2;
        sel.position.y = 0.02;
        sel.visible = false;
        built.group.add(sel);
        built.parts.select = sel;

        self.scene.add(built.group);
        self.nodes[id] = built;
      });
    }

    buildEdges() {
      var T = this.T, sim = this.sim;
      this.curves = {};
      var lines = new T.Group();
      var self = this;
      Object.keys(sim.nodes).forEach(function (id) {
        sim.nodes[id].inputs.forEach(function (input) {
          if (!sim.nodes[input]) return;
          var a = sim.positions[input], b = sim.positions[id];
          var from = new T.Vector3(a.x, 1.2, a.z);
          var to = new T.Vector3(b.x, 1.2, b.z);
          var mid = from.clone().lerp(to, 0.5);
          mid.y = 2.4;
          var curve = new T.QuadraticBezierCurve3(from, mid, to);
          self.curves[input + "->" + id] = curve;
          var geo = new T.BufferGeometry().setFromPoints(curve.getPoints(40));
          var line = new T.Line(geo, new T.LineBasicMaterial({ color: C.ink, transparent: true, opacity: 0.16 }));
          lines.add(line);
        });
      });
      this.scene.add(lines);
    }

    frameCamera() {
      var T = this.T, sim = this.sim;
      var xs = [], zs = [];
      Object.keys(sim.positions).forEach(function (id) {
        xs.push(sim.positions[id].x); zs.push(sim.positions[id].z);
      });
      var cx = (Math.min.apply(null, xs) + Math.max.apply(null, xs)) / 2;
      var cz = (Math.min.apply(null, zs) + Math.max.apply(null, zs)) / 2;
      var span = Math.max(Math.max.apply(null, xs) - Math.min.apply(null, xs), 6);
      this.orbit.target = new T.Vector3(cx, 0.9, cz);
      this.orbit.span = span;
      // Bounding sphere of the whole pipeline, node bodies included.
      this.orbit.sceneRadius = span / 2 + 3.6;
      this.orbit.zoom = 1;
    }

    bindInput() {
      var el = this.renderer.domElement, self = this, drag = null;
      el.addEventListener("pointerdown", function (e) {
        drag = { x: e.clientX, y: e.clientY, moved: 0 };
        el.setPointerCapture(e.pointerId);
        el.style.cursor = "grabbing";
      });
      el.addEventListener("pointermove", function (e) {
        if (!drag) return;
        var dx = e.clientX - drag.x, dy = e.clientY - drag.y;
        drag.moved += Math.abs(dx) + Math.abs(dy);
        drag.x = e.clientX; drag.y = e.clientY;
        self.orbit.manual -= dx * 0.005;
        self.orbit.tilt = Math.max(-0.5, Math.min(1.2, (self.orbit.tilt || 0) + dy * 0.004));
      });
      el.addEventListener("pointerup", function (e) {
        el.style.cursor = "grab";
        if (drag && drag.moved < 6) self.pick(e);
        drag = null;
      });
      el.addEventListener("wheel", function (e) {
        e.preventDefault();
        self.orbit.zoom = Math.max(0.45, Math.min(2.2, self.orbit.zoom + e.deltaY * 0.0012));
      }, { passive: false });
    }

    pick(e) {
      var T = this.T;
      var rect = this.renderer.domElement.getBoundingClientRect();
      var ndc = new T.Vector2(
        ((e.clientX - rect.left) / rect.width) * 2 - 1,
        -((e.clientY - rect.top) / rect.height) * 2 + 1
      );
      this._ray = this._ray || new T.Raycaster();
      this._ray.setFromCamera(ndc, this.camera);
      var hits = [];
      var self = this;
      Object.keys(this.nodes).forEach(function (id) { hits.push(self.nodes[id].hit); });
      var found = this._ray.intersectObjects(hits, false);
      this.sim.select(found.length ? found[0].object.userData.nodeId : null);
    }

    /* ---------- per-frame ---------- */

    entityFor(job) {
      var e = this.entities.get(job.job_id);
      if (e) return e;
      var T = this.T;
      e = this.pool.pop();
      if (!e) {
        e = new T.Mesh(this.geo.entity, this.mats.active);
        this.entityLayer.add(e);
      }
      e.visible = true;
      this.entities.set(job.job_id, e);
      return e;
    }

    stallLevel(ageMs, phase) {
      if (phase === "abandoned") return 3;
      if (ageMs > 60000) return 3;
      if (ageMs > 30000) return 2;
      if (ageMs > this.sim.stallThresholdMs) return 1;
      return 0;
    }

    updateEntities(now, t) {
      var sim = this.sim, jobs = sim.jobList(), seen = new Set();
      var perNode = {}, queuedPerNode = {};

      for (var i = 0; i < jobs.length; i++) {
        var job = jobs[i];
        var e = this.entityFor(job);
        seen.add(job.job_id);
        var age = sim.ageOf(job, now);
        var level = this.stallLevel(age, job.phase);

        if (job.mode === "travel" && job.travel) {
          var curve = this.curves[job.travel.from + "->" + job.travel.to];
          var p = Math.min(1, (now - job.travel.start) / job.travel.dur);
          if (curve) curve.getPoint(p, e.position);
          e.material = this.mats.active;
          e.scale.setScalar(1);
          e.rotation.x = t * 1.6; e.rotation.y = t * 1.1;
        } else {
          var node = this.nodes[job.node];
          if (!node) continue;
          var base = node.group.position;
          if (job.queued) {
            // Backpressure made physical: queued work orbits outside the node,
            // the ring widening as the queue deepens.
            var qi = (queuedPerNode[job.node] = (queuedPerNode[job.node] || 0) + 1) - 1;
            var qn = Math.max(1, sim.nodes[job.node].counters.queue_depth);
            var radius = 2.5 + Math.min(2.6, qn * 0.075);
            var qa = (qi / qn) * Math.PI * 2 + t * 0.25;
            e.position.set(base.x + Math.cos(qa) * radius, 0.75, base.z + Math.sin(qa) * radius);
            e.material = this.mats.queued;
            e.scale.setScalar(0.8);
            e.rotation.y = t * 0.4;
          } else {
            var ni = (perNode[job.node] = (perNode[job.node] || 0) + 1) - 1;
            var nn = Math.max(1, sim.nodes[job.node].counters.in_flight + (job.phase === "abandoned" ? 1 : 0));
            var ang = (ni / Math.max(3, nn)) * Math.PI * 2;
            var r = 1.0 + (ni % 3) * 0.16;
            var moving = job.phase === "active";
            var spin = moving ? t * 0.9 : 0; // a stalled item simply stops
            e.position.set(
              base.x + Math.cos(ang + spin) * r,
              1.45 + Math.sin(ang * 2 + (moving ? t : 0)) * 0.18,
              base.z + Math.sin(ang + spin) * r
            );
            if (job.phase === "abandoned") {
              e.material = level >= 3 && Math.sin(t * 9) > 0.4 ? this.mats.faultWire : this.mats.fault;
              e.scale.setScalar(0.95 + Math.sin(t * 3.2 + ni) * 0.06);
            } else if (job.phase === "held") {
              var loud = level >= 3;
              e.material = loud && Math.sin(t * 8) > 0.2 ? this.mats.faultWire : this.mats.held;
              var puls = level === 0 ? 0 : level === 1 ? 0.05 : level === 2 ? 0.12 : 0.22;
              e.scale.setScalar(1 + Math.sin(t * (2 + level * 1.6)) * puls);
            } else {
              e.material = this.mats.active;
              e.scale.setScalar(1);
            }
            e.rotation.x = moving ? t * 1.2 : 0.6;
            e.rotation.y = moving ? t * 0.8 : 0.4;
          }
        }
      }

      var self = this;
      this.entities.forEach(function (mesh, id) {
        if (seen.has(id)) return;
        mesh.visible = false;
        self.entities.delete(id);
        self.pool.push(mesh);
      });
    }

    updateNodes(now, t, dt) {
      var sim = this.sim, T = this.T;
      var detail = this.mode === "detail";
      for (var id in this.nodes) {
        var built = this.nodes[id], state = sim.nodes[id], p = built.parts;
        var rate = state.counters.throughput_per_sec;

        var worstAge = 0, worstPhase = "active";
        var jobs = sim.jobList();
        for (var i = 0; i < jobs.length; i++) {
          var j = jobs[i];
          if (j.node !== id || j.mode === "travel") continue;
          if (j.phase === "active") continue;
          var age = sim.ageOf(j, now);
          if (age > worstAge) { worstAge = age; worstPhase = j.phase; }
        }
        var level = worstAge ? this.stallLevel(worstAge, worstPhase) : 0;
        var accent = level === 0 ? C.ink : level >= 3 ? C.fault : C.signal;

        p.warn.visible = level >= 2;
        if (p.warn.visible) {
          p.warn.material.emissive.setHex(accent);
          p.warn.material.emissiveIntensity = level >= 3 ? 1.4 + Math.sin(t * 7) * 0.6 : 0.9;
          p.warn.rotation.y = t * (level >= 3 ? 2.4 : 0.8);
          p.warn.position.y = 2.85 + Math.sin(t * 2) * 0.06;
        }

        p.select.visible = sim.selected === id;
        if (p.select.visible) p.select.material.emissive.setHex(accent);

        if (built.kind === "portal") {
          var speed = 0.25 + Math.min(2.2, rate * 0.5);
          p.rings[0].rotation.z += dt * speed * 0.6;
          p.rings[1].rotation.z -= dt * speed;
          p.rings[2].rotation.z += dt * speed * 1.5;
          p.rings[1].material.emissive.setHex(accent);
          p.rings[2].material.emissive.setHex(accent);
          p.core.material.color.setHex(accent);
          p.core.material.opacity = 0.14 + Math.min(0.24, rate * 0.05) + Math.sin(t * 1.5) * 0.03;
          p.fins.rotation.x = t * 0.12;
        } else if (built.kind === "prism") {
          p.core.rotation.y += dt * (0.3 + Math.min(1.4, rate * 0.25));
          p.core.rotation.x += dt * 0.12;
          p.coreWire.rotation.copy(p.core.rotation);
          p.core.material.emissive.setHex(accent);
          p.core.material.emissiveIntensity = 0.3 + Math.min(0.5, rate * 0.1);
          p.shardRig.rotation.y -= dt * (0.4 + Math.min(1.2, rate * 0.2));
          for (var s = 0; s < p.shards.length; s++) {
            p.shards[s].rotation.y += dt * 1.4;
            p.shards[s].material.emissive.setHex(accent);
            p.shards[s].position.y = Math.sin(t * 1.2 + s * 2) * 0.22;
          }
          p.ring.material.emissive.setHex(accent);
          p.ring.rotation.z = t * 0.2;
          // Processing pressure: fins ride up as p50 climbs.
          var lift = Math.min(0.6, state.counters.p50_ms / 900);
          for (var f = 0; f < p.fins.length; f++) p.fins[f].scale.y = 1 + lift;
        } else {
          var pulse = state._commitPulse ? Math.max(0, 1 - (now - state._commitPulse) / 900) : 0;
          for (var k = 0; k < p.tiles.length; k++) {
            var tile = p.tiles[k];
            var wave = Math.max(0, pulse - tile.userData.d * 0.18);
            tile.material.emissive.setHex(accent);
            tile.material.emissiveIntensity = wave * 1.6 + (detail ? 0.04 : 0.02);
            tile.position.y = 0.035 + wave * 0.12;
          }
          p.steps[1].rotation.y += dt * 0.1;
          p.steps[2].rotation.y -= dt * 0.16;
          p.pool.material.opacity = 0.05 + pulse * 0.14;
        }
      }
    }

    loop() {
      if (this._dead) return;
      requestAnimationFrame(this.loop.bind(this));
      var now = performance.now();
      var dt = Math.min(0.05, (now - this.last) / 1000);
      this.last = now;
      if (this.hidden_) return;
      this.frame(now, dt);
    }

    frame(now, dt) {
      var t = now / 1000;
      var detail = this.mode === "detail";
      // The pipeline is a thin horizontal line in a tall frame; a readable floor
      // is what stops the surplus space reading as void instead of depth.
      this.grid.material.opacity = detail ? 0.3 : 0.22;

      var o = this.orbit, T = this.T;
      o.angle += dt * (detail ? 0.035 : 0.075);
      // A pipeline is a line: sweeping a limited arc around broadside keeps every
      // stage visible instead of periodically staring down the axis.
      var a = -Math.PI / 2 + Math.sin(o.angle) * 0.22 + o.manual;
      var pads = this.pads;
      var W = this.clientWidth || 1, H = this.clientHeight || 1;
      var margin = 26;
      var freeW = Math.max(120, W - pads.l - pads.r - margin * 2);
      var freeH = Math.max(120, H - pads.t - pads.b - margin * 2);

      var tan = Math.tan((this.camera.fov * Math.PI / 180) / 2);
      var aspect = W / H;

      // The whole pipeline stays in frame; selecting a stage only lights it. The
      // easing exists so the fit re-settles smoothly when the detail panel opens.
      var wantX = o.target.x, wantZ = o.target.z, wantY = 0.9;
      var wantHalfW = o.span / 2 + 3.4;
      var wantHalfV = 3.2;

      o.look = o.look || new T.Vector3(o.target.x, o.target.y, o.target.z);
      o.halfW = o.halfW === undefined ? wantHalfW : o.halfW;
      o.halfV = o.halfV === undefined ? wantHalfV : o.halfV;
      var ease = Math.min(1, dt * 3.2);
      o.look.lerp(new T.Vector3(wantX, wantY, wantZ), ease);
      o.halfW += (wantHalfW - o.halfW) * ease;
      o.halfV += (wantHalfV - o.halfV) * ease;

      // Fit the scene's flat, wide extent rather than a fat bounding sphere.
      var dist = Math.max(
        o.halfV / (tan * (freeH / H)),
        o.halfW / (tan * aspect * (freeW / W))
      ) * o.zoom;
      var height = dist * Math.max(0.12, Math.min(0.72, 0.32 + (o.tilt || 0) * 0.2));
      var radius = Math.sqrt(Math.max(1, dist * dist - height * height));
      // Fog is set against viewing distance so pulling back never greys the scene out.
      this.scene.fog.density = (detail ? 0.5 : 0.75) / dist;

      this.camera.position.set(
        o.look.x + Math.cos(a) * radius,
        height,
        o.look.z + Math.sin(a) * radius
      );

      // Slide the image so the scene sits in the gap between HUD panels.
      var dxPx = (pads.l + margin + freeW / 2) - W / 2;
      var dyPx = (pads.t + margin + freeH / 2) - H / 2;
      var worldPerPx = (2 * dist * tan) / H;
      var dir = new T.Vector3().subVectors(o.look, this.camera.position).normalize();
      var right = new T.Vector3().crossVectors(dir, new T.Vector3(0, 1, 0)).normalize();
      var up = new T.Vector3().crossVectors(right, dir).normalize();
      var shift = right.multiplyScalar(-dxPx * worldPerPx).add(up.multiplyScalar(dyPx * worldPerPx));
      this.camera.position.add(shift);
      this.camera.lookAt(o.look.clone().add(shift));

      this.updateNodes(now, t, dt);
      this.updateEntities(now, t);
      this.renderer.render(this.scene, this.camera);
    }
  }

  if (!customElements.get("pipeline-stage")) customElements.define("pipeline-stage", PipelineStage);
})();
