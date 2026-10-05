const canvas = document.querySelector("#shell");
const status = document.querySelector("#status");
const context = canvas.getContext("2d", { alpha: false });
const MAX_PIXELS = 8_294_400;
const MAX_EDGE = 8192;

let api;
let image = null;
let pendingFrame = false;
let closed = false;
let width = 1;
let height = 1;

function send(code, a = 0, b = 0, c = 0, d = 0, e = 0) {
  if (closed || !api) return;
  const result = api.sse_web_event(code, a, b, c, d, e);
  if (result === 2) throw new Error(`unsupported browser event ${code}`);
  if (result === 3) throw new Error("WebAssembly runtime is unavailable");
  if (result === 1) {
    closed = true;
    status.hidden = false;
    status.textContent = "Editor closed.";
    return;
  }
  scheduleFrame();
}

function scheduleFrame() {
  if (pendingFrame) return;
  pendingFrame = true;
  requestAnimationFrame(draw);
}

function draw() {
  pendingFrame = false;
  if (closed) return;
  const pointer = api.sse_web_render(width, height) >>> 0;
  const pixelCount = width * height;
  if (!pointer || !api.memory) throw new Error("WebAssembly did not return its shared frame");
  if (!image || image.width !== width || image.height !== height) {
    image = context.createImageData(width, height);
  }

  // The u32 frame is 0xAARRGGBB; wasm memory stores its bytes as BB GG RR AA.
  const source = new Uint8Array(api.memory.buffer, pointer, pixelCount * 4);
  const destination = image.data;
  for (let input = 0, output = 0; input < source.length; input += 4, output += 4) {
    destination[output] = source[input + 2];
    destination[output + 1] = source[input + 1];
    destination[output + 2] = source[input];
    destination[output + 3] = source[input + 3];
  }
  context.putImageData(image, 0, 0);
}

function resize() {
  const bounds = canvas.getBoundingClientRect();
  const deviceScale = Math.min(2, Math.max(1, window.devicePixelRatio || 1));
  let nextWidth = Math.max(1, Math.round(bounds.width * deviceScale));
  let nextHeight = Math.max(1, Math.round(bounds.height * deviceScale));
  const edgeScale = Math.min(1, MAX_EDGE / nextWidth, MAX_EDGE / nextHeight);
  nextWidth = Math.max(1, Math.floor(nextWidth * edgeScale));
  nextHeight = Math.max(1, Math.floor(nextHeight * edgeScale));
  const pixels = nextWidth * nextHeight;
  if (pixels > MAX_PIXELS) {
    const scale = Math.sqrt(MAX_PIXELS / pixels);
    nextWidth = Math.max(1, Math.floor(nextWidth * scale));
    nextHeight = Math.max(1, Math.floor(nextHeight * scale));
  }
  width = nextWidth;
  height = nextHeight;
  canvas.width = width;
  canvas.height = height;
  image = null;
  send(0, width, height);
}

function pointerPosition(event) {
  const bounds = canvas.getBoundingClientRect();
  const displayWidth = Math.max(1, bounds.width);
  const displayHeight = Math.max(1, bounds.height);
  return [
    Math.floor((event.clientX - bounds.left) * width / displayWidth),
    Math.floor((event.clientY - bounds.top) * height / displayHeight),
  ];
}

function keyCode(event) {
  const named = {
    Escape: 0xff1b,
    ArrowUp: 0xff52,
    ArrowDown: 0xff54,
    ArrowLeft: 0xff51,
    ArrowRight: 0xff53,
    Enter: 0xff0d,
    Tab: 0xff09,
    Backspace: 0xff08,
    Delete: 0xffff,
  };
  if (Object.hasOwn(named, event.key)) return [named[event.key], 0];
  const characters = Array.from(event.key);
  if (characters.length === 1) {
    const codepoint = characters[0].codePointAt(0);
    return [codepoint, codepoint];
  }
  return [0, 0];
}

canvas.addEventListener("pointermove", (event) => {
  const [x, y] = pointerPosition(event);
  send(1, x, y);
});
canvas.addEventListener("pointerleave", () => send(2));
canvas.addEventListener("pointerdown", (event) => {
  canvas.focus();
  canvas.setPointerCapture(event.pointerId);
  const [x, y] = pointerPosition(event);
  send(3, event.button + 1, 1, x, y);
});
canvas.addEventListener("pointerup", (event) => {
  const [x, y] = pointerPosition(event);
  send(3, event.button + 1, 0, x, y);
});
canvas.addEventListener("wheel", (event) => {
  event.preventDefault();
  send(4, Math.sign(event.deltaY));
}, { passive: false });
canvas.addEventListener("keydown", (event) => {
  const [keysym, text] = keyCode(event);
  if (keysym === 0xff1b || keysym === 0xff52 || keysym === 0xff54) event.preventDefault();
  send(5, 1, keysym, text, Number(event.ctrlKey), Number(event.shiftKey));
});
canvas.addEventListener("keyup", (event) => {
  const [keysym, text] = keyCode(event);
  send(5, 0, keysym, text, Number(event.ctrlKey), Number(event.shiftKey));
});
canvas.addEventListener("focus", () => send(6, 1));
canvas.addEventListener("blur", () => send(6, 0));
window.addEventListener("resize", resize);
window.addEventListener("pagehide", () => send(7));

try {
  const response = await fetch("./sse_web.wasm");
  if (!response.ok) throw new Error(`could not load wasm (${response.status})`);
  const bytes = await response.arrayBuffer();
  const { instance } = await WebAssembly.instantiate(bytes, {});
  api = instance.exports;
  if (api.sse_web_init() !== 0) throw new Error("could not initialize editor fonts or shell");
  status.hidden = true;
  canvas.focus();
  resize();
} catch (error) {
  status.textContent = `Editor could not start: ${error.message}`;
}
