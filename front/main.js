// Use ES module import syntax to import functionality from the module
// that we have compiled.
  //
  // Note that the `default` import is an initialization function which
// will "boot" the module and make it ready to use. Currently browsers
// don't support natively imported WebAssembly as an ES module, but
// eventually the manual initialization won't be required!
import init, { initialize, initThreadPool, start } from './pkg/firesim.js';

var sim = null;
var currentParams = null;
var baseTickRate = 0;
var paused = false;

function resizeCanvas(canvas) {
  const dpr = window.devicePixelRatio || 1;
  const rect = canvas.getBoundingClientRect();

  const width  = Math.round(rect.width  * dpr);
  const height = Math.round(rect.height * dpr);

  if (canvas.width !== width || canvas.height !== height) {
    canvas.width  = width;
    canvas.height = height;

    const ctx = canvas.getContext("2d");
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }
}

async function run() {
  await init();
  await initThreadPool(navigator.hardwareConcurrency);
  await initialize();
  let canvas = document.getElementById("sim-surface");
  resizeCanvas(canvas);
  canvas.addEventListener("resize", () => resizeCanvas(canvas));
  sim = await start();
  globalThis.sim = sim;
  currentParams = sim.get_parameters();
  baseTickRate = currentParams.tick_rate;
}

run();

function stopButtonClicked() {
  let stats = sim.stop();
  let stats_text = stats.segments.map(s => s.name.toString() + ": " + s.millis.toPrecision(2).toString()).join("\n") + "\nTotal: " + stats.average_step_exec_time.toPrecision(2);
  let stats_panel = document.getElementById("stats");
  stats_panel.style.display = "block";
  let end = document.createElement("p");
  end.innerText = stats_text;
  stats_panel.appendChild(end);
}

document.getElementById("stopButton").addEventListener("click", stopButtonClicked);

function togglePaused() {
  if (paused) {
    paused = false;
    currentParams.tick_rate = baseTickRate;
    sim.set_parameters(currentParams);
    currentParams = sim.get_parameters();
  } else {
    paused = true;
    currentParams.tick_rate = 0.0;
    sim.set_parameters(currentParams);
    currentParams = sim.get_parameters();
  }
}

document.getElementById("pauseButton").addEventListener("click", togglePaused);
