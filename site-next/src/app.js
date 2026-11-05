/* BadTerm's website: a small ANSI terminal in the page.
 *
 * The hero is a real 80x25 text screen: an ANSI-BBS parser (SGR, cursor moves,
 * erase, iCE colors, SAUCE), drawn cell by cell in the IBM VGA 8x16 font BadTerm
 * itself uses, then shown through WebGL: sharp scaling with phosphor mode off,
 * a glow, scanlines and a touch of curve with it on. It dials badterm.com,
 * receives the logo at the chosen line speed, and then takes the menu's keys.
 * Everything else on the page is plain HTML; this only adds to it.
 */
(() => {
  "use strict";

  const $ = (s, el = document) => el.querySelector(s);
  const $$ = (s, el = document) => Array.from(el.querySelectorAll(s));
  const reduced = window.matchMedia("(prefers-reduced-motion: reduce)");
  const store = {
    get(k) { try { return localStorage.getItem(k); } catch (e) { return null; } },
    set(k, v) { try { localStorage.setItem(k, v); } catch (e) { /* private mode */ } },
  };

  // ------------------------------------------------------------ platform
  function detectOS() {
    const p = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "";
    const ua = navigator.userAgent || "";
    if (/iPhone|iPad|iPod|Android/i.test(ua)) return null;
    if (/Win/i.test(p) || /Windows/i.test(ua)) return "windows";
    if (/Mac/i.test(p) || /Mac OS X/i.test(ua)) return "macos";
    if (/Linux|X11|CrOS/i.test(p) || /Linux/i.test(ua)) return "linux";
    return null;
  }
  const OS = detectOS();
  const MOD = OS === "macos" ? "Cmd" : "Alt";

  // ------------------------------------------------------------ the VGA
  const FONT = Uint8Array.from(atob("__VGA_FONT__"), (c) => c.charCodeAt(0));
  const RGB = [0x000000, 0x0000AA, 0x00AA00, 0x00AAAA, 0xAA0000, 0xAA00AA, 0xAA5500, 0xAAAAAA,
               0x555555, 0x5555FF, 0x55FF55, 0x55FFFF, 0xFF5555, 0xFF55FF, 0xFFFF55, 0xFFFFFF];
  // As 32-bit pixels for ImageData (little-endian: 0xAABBGGRR).
  const PIX = RGB.map((c) => (0xFF000000 | ((c & 0xFF) << 16) | (c & 0xFF00) | ((c >>> 16) & 0xFF)) >>> 0);
  const FROM_ANSI = [0, 4, 2, 6, 1, 5, 3, 7]; // SGR 30-37 order -> VGA attribute order
  const cp437 = (() => {
    const low = "\u0000☺☻♥♦♣♠•◘○◙♂♀♪♫☼►◄↕‼¶§▬↨↑↓→←∟↔▲▼";
    const high = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■ ";
    const map = new Map();
    for (let i = 1; i < 32; i++) map.set(low[i], i);
    for (let i = 0; i < 128; i++) map.set(high[i], 128 + i);
    map.set("⌂", 0x7F);
    // ASCII (control codes too: ESC, CR, LF) as itself; the rest through the table.
    return (s) => Array.from(s, (ch) => { const c = ch.codePointAt(0); return c < 127 ? c : (map.get(ch) ?? 0x3F); });
  })();

  class Term {
    constructor(cols, rows) {
      this.cols = cols; this.rows = rows;
      this.ch = new Uint8Array(cols * rows);
      this.at = new Uint8Array(cols * rows);
      this.dirty = new Uint8Array(cols * rows);
      this.reset();
    }
    reset() {
      this.ch.fill(32); this.at.fill(0x07); this.dirty.fill(1);
      this.x = 0; this.y = 0; this.fg = 7; this.bg = 0;
      this.bold = false; this.blink = false; this.rev = false;
      this.ice = true; this.wrap = false; this.cursor = true; this.eof = false;
      this.st = 0; this.params = ""; this.saved = [0, 0];
    }
    attr() {
      let f = this.fg + (this.bold ? 8 : 0), b = this.bg + (this.blink && this.ice ? 8 : 0);
      if (this.rev) [f, b] = [b, f];
      return (b << 4) | f;
    }
    set(x, y, c, a) {
      const i = y * this.cols + x;
      if (this.ch[i] !== c || this.at[i] !== a) { this.ch[i] = c; this.at[i] = a; this.dirty[i] = 1; }
    }
    erase(from, to) {
      const a = (this.attr() & 0xF0) | 0x07;
      for (let i = from; i < to; i++) if (this.ch[i] !== 32 || this.at[i] !== a) { this.ch[i] = 32; this.at[i] = a; this.dirty[i] = 1; }
    }
    write(bytes) { for (let i = 0; i < bytes.length; i++) this.byte(bytes[i]); }
    byte(b) {
      if (this.eof) return;
      if (this.st === 0) {
        if (b === 0x1B) { this.st = 1; return; }
        if (b === 0x0D) { this.x = 0; this.wrap = false; return; }
        if (b === 0x0A) { this.lf(); this.wrap = false; return; }
        if (b === 0x08) { if (this.x > 0) this.x--; this.wrap = false; return; }
        if (b === 0x09) { this.x = Math.min(this.cols - 1, (this.x + 8) & ~7); return; }
        if (b === 0x1A) { this.eof = true; return; } // SAUCE follows
        if (b === 0x07 || b === 0x00) return;
        this.put(b);
        return;
      }
      if (this.st === 1) {
        if (b === 0x5B) { this.st = 2; this.params = ""; return; }
        this.st = 0;
        return;
      }
      if (b >= 0x30 && b <= 0x3F) { this.params += String.fromCharCode(b); return; }
      if (b >= 0x20 && b <= 0x2F) return;
      this.st = 0;
      this.csi(String.fromCharCode(b));
    }
    put(c) {
      // The last column wraps only when the next character comes (as a VGA BBS terminal does).
      if (this.wrap) { this.x = 0; this.lf(); this.wrap = false; }
      this.set(this.x, this.y, c, this.attr());
      if (this.x === this.cols - 1) this.wrap = true; else this.x++;
    }
    lf() {
      if (this.y < this.rows - 1) { this.y++; return; }
      this.ch.copyWithin(0, this.cols); this.at.copyWithin(0, this.cols);
      this.dirty.fill(1);
      this.erase((this.rows - 1) * this.cols, this.rows * this.cols);
    }
    csi(f) {
      const priv = this.params.startsWith("?");
      const ps = (priv ? this.params.slice(1) : this.params).split(";").map((v) => (v === "" ? NaN : parseInt(v, 10)));
      const n = (i, d) => (Number.isNaN(ps[i]) || ps[i] === undefined ? d : ps[i]);
      const clampX = (v) => Math.max(0, Math.min(this.cols - 1, v));
      const clampY = (v) => Math.max(0, Math.min(this.rows - 1, v));
      const C = this.cols;
      switch (f) {
        case "m": for (const p of ps) this.sgr(Number.isNaN(p) ? 0 : p); break;
        case "H": case "f": this.y = clampY(n(0, 1) - 1); this.x = clampX(n(1, 1) - 1); this.wrap = false; break;
        case "A": this.y = clampY(this.y - Math.max(1, n(0, 1))); this.wrap = false; break;
        case "B": this.y = clampY(this.y + Math.max(1, n(0, 1))); this.wrap = false; break;
        case "C": this.x = clampX(this.x + Math.max(1, n(0, 1))); this.wrap = false; break;
        case "D": this.x = clampX(this.x - Math.max(1, n(0, 1))); this.wrap = false; break;
        case "G": this.x = clampX(n(0, 1) - 1); this.wrap = false; break;
        case "J": {
          const m = n(0, 0), here = this.y * C + this.x;
          if (m === 2) { this.erase(0, C * this.rows); this.x = 0; this.y = 0; }
          else if (m === 1) this.erase(0, here + 1);
          else this.erase(here, C * this.rows);
          break;
        }
        case "K": {
          const m = n(0, 0), row = this.y * C;
          if (m === 2) this.erase(row, row + C);
          else if (m === 1) this.erase(row, row + this.x + 1);
          else this.erase(row + this.x, row + C);
          break;
        }
        case "s": this.saved = [this.x, this.y]; break;
        case "u": [this.x, this.y] = this.saved; this.wrap = false; break;
        case "h": case "l":
          if (priv) for (const p of ps) { if (p === 33) this.ice = f === "h"; if (p === 25) this.cursor = f === "h"; }
          break;
        default: break;
      }
    }
    sgr(p) {
      if (p === 0) { this.fg = 7; this.bg = 0; this.bold = this.blink = this.rev = false; }
      else if (p === 1) this.bold = true;
      else if (p === 2 || p === 22) this.bold = false;
      else if (p === 5 || p === 6) this.blink = true;
      else if (p === 25) this.blink = false;
      else if (p === 7) this.rev = true;
      else if (p === 27) this.rev = false;
      else if (p >= 30 && p <= 37) this.fg = FROM_ANSI[p - 30];
      else if (p === 39) this.fg = 7;
      else if (p >= 40 && p <= 47) this.bg = FROM_ANSI[p - 40];
      else if (p === 49) this.bg = 0;
      else if (p >= 90 && p <= 97) { this.fg = FROM_ANSI[p - 90]; this.bold = true; }
      else if (p >= 100 && p <= 107) { this.bg = FROM_ANSI[p - 100]; this.blink = true; }
    }
  }

  // Cells -> pixels: the 80x25 screen and, under it, BadTerm's status bar.
  class Raster {
    constructor(term) {
      this.term = term;
      this.w = term.cols * 8; this.h = (term.rows + 1) * 16;
      this.canvas = document.createElement("canvas");
      this.canvas.width = this.w; this.canvas.height = this.h;
      this.ctx = this.canvas.getContext("2d");
      this.img = this.ctx.createImageData(this.w, this.h);
      this.px = new Uint32Array(this.img.data.buffer);
      this.status = { ch: new Uint8Array(term.cols), at: new Uint8Array(term.cols), dirty: true };
      this.cursorCell = -1;
      this.blinkOn = true;
    }
    cell(x, y, c, a, under) {
      const fg = PIX[a & 15], bg = PIX[a >> 4], w = this.w, g = c * 16;
      let o = y * 16 * w + x * 8;
      for (let r = 0; r < 16; r++, o += w) {
        let bits = FONT[g + r];
        if (under && r >= 14) bits = 0xFF;
        for (let k = 0; k < 8; k++) this.px[o + k] = bits & (0x80 >> k) ? fg : bg;
      }
    }
    setStatus(text, attr) {
      const codes = cp437(text).slice(0, this.term.cols);
      const s = this.status;
      for (let i = 0; i < this.term.cols; i++) {
        const c = i < codes.length ? codes[i] : 32;
        if (s.ch[i] !== c || s.at[i] !== attr) { s.ch[i] = c; s.at[i] = attr; s.dirty = true; }
      }
    }
    paint() {
      const t = this.term, C = t.cols;
      let changed = false;
      const want = t.cursor && this.showCursor && this.blinkOn ? t.y * C + Math.min(t.x, C - 1) : -1;
      if (want !== this.cursorCell) {
        if (this.cursorCell >= 0) t.dirty[this.cursorCell] = 1;
        if (want >= 0) t.dirty[want] = 1;
        this.cursorCell = want;
      }
      for (let i = 0; i < t.ch.length; i++) {
        if (!t.dirty[i]) continue;
        t.dirty[i] = 0;
        this.cell(i % C, (i / C) | 0, t.ch[i], t.at[i], i === this.cursorCell);
        changed = true;
      }
      if (this.status.dirty) {
        for (let i = 0; i < C; i++) this.cell(i, t.rows, this.status.ch[i], this.status.at[i], false);
        this.status.dirty = false;
        changed = true;
      }
      if (changed) this.ctx.putImageData(this.img, 0, 0);
      return changed;
    }
  }

  // ------------------------------------------------------------ the glass
  const VERT = "attribute vec2 p; varying vec2 v; void main(){ v = p * 0.5 + 0.5; v.y = 1.0 - v.y; gl_Position = vec4(p, 0.0, 1.0); }";
  const FRAG = `
precision mediump float;
varying vec2 v;
uniform sampler2D t;
uniform vec2 src, dst;
uniform float mode;
vec3 sharp(vec2 uv) {
  vec2 texel = uv * src;
  vec2 scale = max(floor(dst / src), vec2(1.0));
  vec2 f = fract(texel) - 0.5;
  vec2 r = 0.5 - 0.5 / scale;
  vec2 g = (f - clamp(f, -r, r)) * scale + 0.5;
  return texture2D(t, (floor(texel) + g) / src).rgb;
}
vec3 blur(vec2 uv) {
  vec2 d = 1.0 / src;
  vec3 s = texture2D(t, uv + vec2(-1.5, 0.0) * d).rgb + texture2D(t, uv + vec2(1.5, 0.0) * d).rgb
         + texture2D(t, uv + vec2(0.0, -1.2) * d).rgb + texture2D(t, uv + vec2(0.0, 1.2) * d).rgb;
  vec3 w = texture2D(t, uv + vec2(-3.0, -2.0) * d).rgb + texture2D(t, uv + vec2(3.0, 2.0) * d).rgb
         + texture2D(t, uv + vec2(3.0, -2.0) * d).rgb + texture2D(t, uv + vec2(-3.0, 2.0) * d).rgb;
  return s * 0.17 + w * 0.08;
}
void main() {
  vec2 uv = v;
  if (mode < 0.5) { gl_FragColor = vec4(sharp(uv), 1.0); return; }
  vec2 c = uv * 2.0 - 1.0;
  c *= 1.0 + 0.022 * dot(c, c);
  uv = c * 0.5 + 0.5;
  if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) { gl_FragColor = vec4(0.0, 0.0, 0.0, 1.0); return; }
  vec3 base = sharp(uv);
  vec3 g = blur(uv);
  vec3 col = max(base, g * 0.9) + g * 0.18;
  if (dst.y / src.y >= 1.8) {
    float l = sin(3.14159265 * fract(uv.y * src.y));
    col *= mix(0.66, 1.06, l * l);
  }
  if (mode > 1.5) {
    float y = dot(col, vec3(0.3, 0.59, 0.11));
    vec3 tint = mode < 2.5 ? vec3(0.28, 1.0, 0.48) : vec3(1.0, 0.69, 0.16);
    col = tint * pow(y, 0.85) * 1.12;
  }
  vec2 e = uv - 0.5;
  col *= 1.0 - 0.9 * dot(e * e, vec2(1.0, 1.0));
  float edge = smoothstep(0.0, 0.006, uv.x) * smoothstep(0.0, 0.006, uv.y) * smoothstep(0.0, 0.006, 1.0 - uv.x) * smoothstep(0.0, 0.006, 1.0 - uv.y);
  gl_FragColor = vec4(col * edge, 1.0);
}`;

  class Glass {
    constructor(source) {
      this.source = source;
      this.canvas = document.createElement("canvas");
      const gl = this.canvas.getContext("webgl", { antialias: false, alpha: false, premultipliedAlpha: false });
      this.gl = gl;
      if (!gl) return;
      const sh = (type, src) => {
        const s = gl.createShader(type); gl.shaderSource(s, src); gl.compileShader(s);
        if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
        return s;
      };
      try {
        const prog = gl.createProgram();
        gl.attachShader(prog, sh(gl.VERTEX_SHADER, VERT));
        gl.attachShader(prog, sh(gl.FRAGMENT_SHADER, FRAG));
        gl.linkProgram(prog);
        if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(prog));
        gl.useProgram(prog);
        this.u = { src: gl.getUniformLocation(prog, "src"), dst: gl.getUniformLocation(prog, "dst"), mode: gl.getUniformLocation(prog, "mode") };
        const buf = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, buf);
        gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
        const loc = gl.getAttribLocation(prog, "p");
        gl.enableVertexAttribArray(loc);
        gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
        this.tex = gl.createTexture();
        gl.bindTexture(gl.TEXTURE_2D, this.tex);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        this.ok = true;
      } catch (e) {
        this.ok = false;
      }
    }
    fit() {
      const r = this.canvas.getBoundingClientRect(), dpr = window.devicePixelRatio || 1;
      const w = Math.max(1, Math.round(r.width * dpr)), h = Math.max(1, Math.round(r.height * dpr));
      if (this.canvas.width !== w || this.canvas.height !== h) { this.canvas.width = w; this.canvas.height = h; return true; }
      return false;
    }
    draw(mode, upload) {
      const gl = this.gl;
      if (upload) gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, this.source);
      gl.viewport(0, 0, this.canvas.width, this.canvas.height);
      gl.uniform2f(this.u.src, this.source.width, this.source.height);
      gl.uniform2f(this.u.dst, this.canvas.width, this.canvas.height);
      gl.uniform1f(this.u.mode, mode);
      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    }
  }

  // ------------------------------------------------------------ the call
  const MODES = { off: 0, color: 1, green: 2, amber: 3 };
  const STATUS_ATTR = (3 << 4) | 0; // black on cyan

  function hero() {
    const screen = $("#screen");
    if (!screen) return;
    const term = new Term(80, 25);
    const raster = new Raster(term);
    const glass = new Glass(raster.canvas);
    const view = glass.ok ? glass.canvas : raster.canvas;
    if (!glass.ok) view.classList.add("px");
    view.setAttribute("aria-hidden", "true");
    let hotspots = [], prompt = null;
    try { hotspots = JSON.parse(screen.dataset.hotspots || "[]"); prompt = JSON.parse(screen.dataset.prompt || "null"); } catch (e) { /* none */ }
    const monitor = $("#monitor");
    let mode = store.get("badterm-crt") || "off";
    if (!(mode in MODES)) mode = "off";
    let baud = parseInt(store.get("badterm-baud") || "33600", 10);
    const baudSel = $("#baud");
    if (baudSel && [...baudSel.options].some((o) => +o.value === baud)) baudSel.value = String(baud); else baud = 33600;

    // The call: a queue of steps run against the clock.
    let steps = [], art = null, started = 0, online = -1, done = false, needsDraw = true, uploaded = false;
    const enc = (s) => Uint8Array.from(cp437(s));
    const status = {
      dialing: () => raster.setStatus(" Connecting to badterm.com...  " + MOD + "-H cancel", STATUS_ATTR),
      online: (secs) => {
        const hh = String(Math.floor(secs / 3600)).padStart(2, "0"), mm = String(Math.floor(secs / 60) % 60).padStart(2, "0"), ss = String(secs % 60).padStart(2, "0");
        raster.setStatus(` badterm.com │ ${hh}:${mm}:${ss} iCE │ ${MOD}: D Dir H Hangup U Upload Z Help X Exit`, STATUS_ATTR);
      },
      idle: () => raster.setStatus(" Not connected │ " + MOD + "-D Dialing directory  " + MOD + "-Z Help  " + MOD + "-X Exit", STATUS_ATTR),
    };

    function plan() {
      const rate = baud / 10;
      return [
        { fn: () => { term.reset(); term.cursor = true; raster.showCursor = true; status.dialing(); online = -1; done = false; screen.classList.remove("has-link"); } },
        { bytes: enc("\x1b[0m\x1b[2J\x1b[37m"), rate: Infinity },
        { bytes: enc("ATDT badterm.com"), rate: 26 },
        { wait: 500 },
        { bytes: enc("\r\n"), rate: Infinity },
        { wait: 1100 },
        { bytes: enc(`\r\nCONNECT ${baud}/ARQ/V42BIS\r\n`), rate: Infinity },
        { wait: 450 },
        { fn: () => { online = performance.now(); status.online(0); } },
        { bytes: art, rate },
        { fn: finish },
      ];
    }
    function finish() {
      if (prompt) { term.x = prompt.col; term.y = prompt.row; term.wrap = false; }
      done = true;
      $("#hint").textContent = "Click the screen and press a key.";
    }
    function skipAll() {
      for (const s of steps) { if (s.fn) s.fn(); else if (s.bytes) term.write(s.bytes.subarray(s.pos || 0)); }
      steps = [];
      needsDraw = true;
    }
    function run(now) {
      while (steps.length) {
        const s = steps[0];
        if (s.fn) { s.fn(); steps.shift(); continue; }
        if (s.wait !== undefined) {
          s.until = s.until ?? now + s.wait;
          if (now < s.until) return;
          steps.shift(); continue;
        }
        if (s.bytes) {
          s.pos = s.pos || 0; s.t0 = s.t0 ?? now;
          const want = s.rate === Infinity ? s.bytes.length : Math.min(s.bytes.length, Math.floor((now - s.t0) / 1000 * s.rate) + 1);
          if (want > s.pos) { term.write(s.bytes.subarray(s.pos, want)); s.pos = want; needsDraw = true; }
          if (s.pos >= s.bytes.length) { steps.shift(); continue; }
          return;
        }
        steps.shift();
      }
    }
    function dial() {
      steps = plan();
      started = performance.now();
      if (reduced.matches) skipAll();
      $("#hint").textContent = reduced.matches ? "Click the screen and press a key." : "Dialing... press any key to skip.";
    }

    // Drawing: only when something changed, the cursor blinks, or the size or mode did.
    let lastSecs = -1;
    // One step of the call and one picture. Animation frames drive it; a slow
    // timer stands in when the browser stops sending them (a quiet screen, a
    // background tab), so the call never stalls.
    let lastTick = 0;
    function tick(now) {
      lastTick = now;
      run(now);
      const blink = Math.floor(now / 420) % 2 === 0;
      if (blink !== raster.blinkOn) { raster.blinkOn = blink; needsDraw = true; }
      if (online >= 0) {
        const secs = Math.floor((now - online) / 1000);
        if (secs !== lastSecs) { lastSecs = secs; status.online(secs); needsDraw = true; }
      }
      const changed = raster.paint();
      if (glass.ok) {
        const resized = glass.fit();
        if (changed || resized || needsDraw) glass.draw(MODES[mode], changed || !uploaded);
        uploaded = true;
      }
      needsDraw = false;
    }
    // performance.now() for every clock reading: a frame's own timestamp can run on another clock.
    function frame() { tick(performance.now()); requestAnimationFrame(frame); }
    function backstop() { const now = performance.now(); if (now - lastTick > 150) tick(now); }

    function setMode(m) {
      mode = m;
      store.set("badterm-crt", m);
      for (const b of $$("[data-crt]")) b.setAttribute("aria-pressed", String(b.dataset.crt === m));
      monitor.classList.remove("crt-color", "crt-green", "crt-amber");
      if (m !== "off") monitor.classList.add("crt-" + m);
      if (!glass.ok && m !== "off") $("#hint").textContent = "Phosphor mode needs WebGL, which this browser hasn't got.";
      needsDraw = true;
    }

    // The menu's keys and the clicks on it.
    function act(key) {
      const k = key.toUpperCase();
      if (k === "R") { dial(); return true; }
      const spot = hotspots.find((h) => h.key === k);
      if (!spot) return false;
      if (done) { term.put(k.charCodeAt(0)); needsDraw = true; }
      setTimeout(() => {
        if (k === "P") {
          const order = ["off", "color", "green", "amber"];
          setMode(order[(order.indexOf(mode) + 1) % order.length]);
          if (done && prompt) { term.x = prompt.col; term.y = prompt.row; term.erase(prompt.row * 80 + prompt.col, prompt.row * 80 + 80); }
          return;
        }
        const target = document.querySelector(spot.href);
        if (target) target.scrollIntoView({ behavior: reduced.matches ? "auto" : "smooth" });
        if (done && prompt) { term.x = prompt.col; term.y = prompt.row; term.erase(prompt.row * 80 + prompt.col, prompt.row * 80 + 80); }
      }, done ? 220 : 0);
      return true;
    }
    function cellAt(ev) {
      const r = screen.getBoundingClientRect();
      const col = Math.floor((ev.clientX - r.left) / r.width * 80);
      const row = Math.floor((ev.clientY - r.top) / r.height * 26);
      return { col, row };
    }
    screen.addEventListener("keydown", (ev) => {
      if (ev.ctrlKey || ev.metaKey || ev.altKey) return;
      if (steps.length && ev.key !== "Tab") { skipAll(); ev.preventDefault(); return; }
      if (ev.key.length === 1 && act(ev.key)) ev.preventDefault();
    });
    screen.addEventListener("click", (ev) => {
      if (steps.length) { skipAll(); return; }
      const { col, row } = cellAt(ev);
      const spot = hotspots.find((h) => h.row === row && col >= h.c0 && col <= h.c1);
      if (spot) act(spot.key);
      else screen.focus();
    });
    screen.addEventListener("mousemove", (ev) => {
      if (!done) return;
      const { col, row } = cellAt(ev);
      screen.classList.toggle("has-link", hotspots.some((h) => h.row === row && col >= h.c0 && col <= h.c1));
    });
    for (const b of $$("[data-crt]")) b.addEventListener("click", () => setMode(b.dataset.crt));
    if (baudSel) baudSel.addEventListener("change", () => { baud = +baudSel.value; store.set("badterm-baud", String(baud)); dial(); });
    const redial = $("#redial");
    if (redial) redial.addEventListener("click", () => dial());

    // Start: fetch the art, swap the picture for the live screen, dial.
    fetch(screen.dataset.art, { cache: "force-cache" })
      .then((r) => (r.ok ? r.arrayBuffer() : Promise.reject(new Error(r.status))))
      .then((buf) => {
        art = new Uint8Array(buf);
        const img = $(".fallback", screen);
        screen.appendChild(view);
        if (img) img.remove();
        setMode(mode);
        status.idle();
        dial();
        requestAnimationFrame(frame);
        setInterval(backstop, 200);
      })
      .catch(() => { /* the picture stays */ });
  }

  // ------------------------------------------------------------ the rest of the page
  function downloads() {
    if (OS) {
      const card = $(`.os[data-os="${OS}"]`);
      if (card) card.classList.add("mine");
      const get = $("#get");
      const url = get && get.dataset[OS];
      if (url) {
        get.href = url;
        get.textContent = { windows: "Download for Windows", macos: "Download for macOS", linux: "Download for Linux" }[OS];
        if (get.dataset[OS + "Note"]) get.title = get.dataset[OS + "Note"];
      }
    }
    for (const b of $$("[data-copy]")) {
      b.addEventListener("click", () => {
        const done = (ok) => { b.textContent = ok ? "Copied" : "Select it"; setTimeout(() => { b.textContent = "Copy"; }, 1600); };
        if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(b.dataset.copy).then(() => done(true), () => done(false));
        else done(false);
      });
    }
  }

  function compare() {
    const box = $("#compare"), range = $("#compare-range"), b = $("#compare-b"), tag = $("#compare-tag");
    if (!box || !range || !b) return;
    const set = () => { const v = range.value; b.style.clipPath = `inset(0 0 0 ${v}%)`; box.style.setProperty("--split", v + "%"); };
    range.addEventListener("input", set);
    set();
    for (const btn of $$("[data-mon]")) {
      btn.addEventListener("click", () => {
        b.src = btn.dataset.src;
        tag.textContent = btn.dataset.tag;
        b.alt = `The Vortex BBS in phosphor mode on a ${btn.dataset.tag.toLowerCase()} monitor`;
        for (const o of $$("[data-mon]")) o.setAttribute("aria-pressed", String(o === btn));
      });
    }
  }

  // Light up the link to whichever section is in view: the header's links
  // to this page's sections, and the help menu's to the guide's.
  function spy(links, margin) {
    if (!("IntersectionObserver" in window) || !links.length) return;
    const here = location.pathname;
    const map = new Map();
    for (const a of links) {
      const u = new URL(a.getAttribute("href"), location.href);
      if (u.pathname === here && u.hash) map.set(decodeURIComponent(u.hash.slice(1)), a);
    }
    const io = new IntersectionObserver((entries) => {
      for (const e of entries) {
        const a = map.get(e.target.id);
        if (!a || !e.isIntersecting) continue;
        for (const l of map.values()) l.removeAttribute("aria-current");
        a.setAttribute("aria-current", "true");
        if (a.closest(".toc") && a.scrollIntoView) {
          const box = a.closest(".toc"), r = a.getBoundingClientRect(), br = box.getBoundingClientRect();
          if (r.top < br.top || r.bottom > br.bottom) box.scrollTop += r.top - br.top - br.height / 2;
        }
      }
    }, { rootMargin: margin });
    for (const id of map.keys()) { const el = document.getElementById(id); if (el) io.observe(el); }
  }

  hero();
  downloads();
  compare();
  spy($$(".nav ul a"), "-40% 0px -55% 0px");
  spy($$(".toc a"), "-15% 0px -75% 0px");
})();
