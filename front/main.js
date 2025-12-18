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
var paused = false;
var paramSettings;
var originalMonthsPerSecond = null;

function setParameters(params) {
  sim.set_parameters(params);
  currentParams = sim.get_parameters();
}

function updateParams(f) {
  f(currentParams);
  sim.set_parameters(currentParams);
  currentParams = sim.get_parameters();
  updateStaticParamsDisplay();
}

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
  originalMonthsPerSecond = currentParams.months_per_second;
  initParamsPanel();
}

run();

async function stopButtonClicked() {
  let stats = await sim.stop();
  let stats_text = "Average time per step: " + stats.average_step_exec_time.toPrecision(2) + "ms";
  let stats_panel = document.getElementById("stats");
  stats_panel.style.display = "block";
  let end = document.createElement("p");
  end.innerText = stats_text;
  stats_panel.appendChild(end);
}

document.getElementById("stopButton").addEventListener("click", stopButtonClicked);

function togglePaused() {
  updateParams(params => {
    if (paused) {
      paused = false;
      // Restore original months_per_second to resume
      params.months_per_second = originalMonthsPerSecond;
    } else {
      paused = true;
      // Set months_per_second to 0 to pause (tick_rate will be 0)
      params.months_per_second = 0;
    }
  })
}

function initParamsPanel() {
  let static_panel = document.getElementById("static-params");
  let mutable_panel = document.getElementById("mutable-params");
  let p = currentParams;
  
  // Static parameters (read-only)
  const staticParams = [
    'forest_width',
    'forest_height',
    'forest_acres',
  ];
  
  // Mutable parameters (editable)
  const mutableParams = [
    'months_per_second',
    'lightning_strikes_per_year_per_acre',
    'tree_growth_years',
    'tree_death_years',
    'underbrush_tree_growth_hindrance',
    'tree_underbrush_generation',
    'tree_death_underbrush',
    'tree_fire_duration',
    'underbrush_fire_duration',
    'fire_spread_rate',
    'tree_flammability',
    'underbrush_flammability',
    'ticks_per_month'
  ];
  
  // Create static parameters panel (read-only values)
  staticParams.forEach(paramName => {
    let label = document.createElement("p");
    label.innerText = formatParamName(paramName) + ": ";
    let value = document.createElement("span");
    value.style.fontWeight = "bold";
    value.style.marginLeft = "8px";
    value.id = `static-${paramName}`;
    value.innerText = formatParamValue(p[paramName], paramName);
    let newl = document.createElement("br");
    static_panel.appendChild(label);
    static_panel.appendChild(value);
    static_panel.appendChild(newl);
  });
  static_panel.style.display = "block";
  
  // Create mutable parameters panel (editable inputs)
  mutableParams.forEach(paramName => {
    let label = document.createElement("p");
    label.innerText = formatParamName(paramName) + ": ";
    let input = document.createElement("input");
    input.type = "number";
    input.step = getStepForParam(paramName);
    input.value = typeof p[paramName] === 'number' ? p[paramName].toFixed(2) : p[paramName];
    input.id = `mutable-${paramName}`;
    let newl = document.createElement("br");
    mutable_panel.appendChild(label);
    mutable_panel.appendChild(input);
    mutable_panel.appendChild(newl);
    input.addEventListener("input", () => {
      updateParams(params => {
        const numValue = parseFloat(input.value);
        if (!isNaN(numValue)) {
          params[paramName] = numValue; // Update without rounding while editing
        }
      });
      // Update static params display in case they depend on mutable params
      updateStaticParamsDisplay();
    });
    input.addEventListener("blur", () => {
      // Round and constrain value when done editing
      const numValue = parseFloat(input.value);
      if (!isNaN(numValue)) {
        const roundedValue = Math.round(numValue * 100) / 100; // Round to 2 decimal places
        updateParams(params => {
          params[paramName] = roundedValue;
        });
        input.value = roundedValue.toFixed(2); // Update display to show rounded value
        // Update static params display in case they depend on mutable params
        updateStaticParamsDisplay();
        // If months_per_second changed and we're not paused, update original value
        if (paramName === 'months_per_second' && !paused) {
          originalMonthsPerSecond = roundedValue;
        }
      }
    });
  });
  mutable_panel.style.display = "block";
  
  // Store param settings for reference
  paramSettings = [...staticParams, ...mutableParams].map(prop => ({
    name: prop,
    value: p[prop],
  }));
}

function formatParamName(name) {
  return name
    .split('_')
    .map(word => word.charAt(0).toUpperCase() + word.slice(1))
    .join(' ');
}

function formatParamValue(value, paramName) {
  if (paramName === 'forest_acres') {
    return value.toFixed(2) + ' acres';
  }
  if (paramName === 'months_per_second') {
    return value.toFixed(2) + ' months/sec';
  }
  if (paramName === 'ticks_per_month') {
    return value.toFixed(1) + ' ticks/month';
  }
  if (paramName.includes('years')) {
    return value.toFixed(1) + ' years';
  }
  if (paramName.includes('per_year_per_acre')) {
    return value.toFixed(4) + ' strikes/year/acre';
  }
  if (typeof value === 'number') {
    if (Number.isInteger(value)) {
      return value.toString();
    }
    return value.toFixed(4);
  }
  return value.toString();
}

function getStepForParam(paramName) {
  if (paramName.includes('years') || paramName.includes('per_year')) {
    return '0.1';
  }
  if (paramName.includes('duration')) {
    return '1';
  }
  return '0.0001';
}

function updateStaticParamsDisplay() {
  if (!currentParams) return;
  const staticParams = [
    'forest_width',
    'forest_height',
    'forest_acres',
    'ticks_per_month'
  ];
  
  staticParams.forEach(paramName => {
    const element = document.getElementById(`static-${paramName}`);
    if (element) {
      element.innerText = formatParamValue(currentParams[paramName], paramName);
    }
  });
}

document.getElementById("pauseButton").addEventListener("click", togglePaused);
