// Fire Simulation with GPU Rendering
// Uses WebGPU for both compute simulation and rendering

import init, { initialize, start, SimulationController } from './pkg/firesim.js';

let isPaused = false;
let isStopped = false;

function resizeCanvas(canvas) {
  const dpr = window.devicePixelRatio || 1;
  const rect = canvas.getBoundingClientRect();

  const width  = Math.round(rect.width  * dpr);
  const height = Math.round(rect.height * dpr);

  if (canvas.width !== width || canvas.height !== height) {
    canvas.width  = width;
    canvas.height = height;
    // Note: Don't get a 2D context here - WebGPU will handle rendering
  }
}

// Parameter definitions with metadata
const PARAMETERS = {
  months_per_second: {
    label: "Simulation Speed (months/sec)",
    min: 1,
    max: 1000,
    step: 1,
    default: 36,
    setter: (v) => SimulationController.set_months_per_second(v)
  },
  ticks_per_month: {
    label: "Resolution (ticks/month)",
    min: 1,
    max: 20,
    step: 1,
    default: 2,
    setter: (v) => SimulationController.set_ticks_per_month(v)
  },
  lightning_frequency: {
    label: "Lightning (strikes/year/acre)",
    min: 0,
    max: 0.1,
    step: 0.001,
    default: 1/45,
    setter: (v) => SimulationController.set_lightning_frequency(v)
  },
  fire_spread_rate: {
    label: "Fire Spread Rate",
    min: 0,
    max: 1,
    step: 0.05,
    default: 1.0,
    setter: (v) => SimulationController.set_fire_spread_rate(v)
  },
  tree_growth_years: {
    label: "Tree Growth (years)",
    min: 10,
    max: 500,
    step: 10,
    default: 150,
    setter: (v) => SimulationController.set_tree_growth_years(v)
  },
  tree_death_years: {
    label: "Tree Death (years)",
    min: 50,
    max: 500,
    step: 10,
    default: 200,
    setter: (v) => SimulationController.set_tree_death_years(v)
  },
  tree_flammability: {
    label: "Tree Flammability",
    min: 0,
    max: 1,
    step: 0.05,
    default: 0.5,
    setter: (v) => SimulationController.set_tree_flammability(v)
  },
  underbrush_flammability: {
    label: "Underbrush Flammability",
    min: 0,
    max: 2,
    step: 0.1,
    default: 1.0,
    setter: (v) => SimulationController.set_underbrush_flammability(v)
  },
  tree_underbrush_generation: {
    label: "Underbrush Generation",
    min: 0,
    max: 0.01,
    step: 0.00001,
    default: 0.0001,
    setter: (v) => SimulationController.set_tree_underbrush_generation(v)
  },
  underbrush_tree_growth_hindrance: {
    label: "Underbrush Growth Hindrance",
    min: 0,
    max: 1,
    step: 0.1,
    default: 0,
    setter: (v) => SimulationController.set_underbrush_tree_growth_hindrance(v)
  },
  tree_fire_duration: {
    label: "Tree Fire Duration (ticks)",
    min: 1,
    max: 10,
    step: 1,
    default: 1,
    setter: (v) => SimulationController.set_tree_fire_duration(v)
  },
  underbrush_fire_duration: {
    label: "Underbrush Fire Duration (ticks)",
    min: 1,
    max: 10,
    step: 1,
    default: 1,
    setter: (v) => SimulationController.set_underbrush_fire_duration(v)
  }
};

function formatValue(value, step) {
  if (step >= 1) return Math.round(value).toString();
  const decimals = Math.max(0, -Math.floor(Math.log10(step)));
  return value.toFixed(decimals);
}

function createParameterControl(key, param) {
  const container = document.createElement('div');
  container.className = 'param-control';
  
  const label = document.createElement('label');
  label.textContent = param.label;
  label.htmlFor = `param-${key}`;
  
  const inputContainer = document.createElement('div');
  inputContainer.className = 'input-container';
  
  const input = document.createElement('input');
  input.type = 'range';
  input.id = `param-${key}`;
  input.min = param.min;
  input.max = param.max;
  input.step = param.step;
  input.value = param.default;
  
  const valueDisplay = document.createElement('span');
  valueDisplay.className = 'value-display';
  valueDisplay.textContent = formatValue(param.default, param.step);
  
  input.addEventListener('input', (e) => {
    const value = parseFloat(e.target.value);
    valueDisplay.textContent = formatValue(value, param.step);
    param.setter(value);
  });
  
  inputContainer.appendChild(input);
  inputContainer.appendChild(valueDisplay);
  
  container.appendChild(label);
  container.appendChild(inputContainer);
  
  return container;
}

function initParametersPanel() {
  const panel = document.getElementById('mutable-params');
  if (!panel) return;
  
  panel.style.display = 'block';
  
  // Clear existing content except the header
  const header = panel.querySelector('h1');
  panel.innerHTML = '';
  if (header) panel.appendChild(header);
  
  // Add parameter controls
  for (const [key, param] of Object.entries(PARAMETERS)) {
    panel.appendChild(createParameterControl(key, param));
  }
}

function updatePauseButton() {
  const pauseButton = document.getElementById('pauseButton');
  if (pauseButton) {
    if (isStopped) {
      pauseButton.textContent = 'Resume simulation';
    } else {
      pauseButton.textContent = isPaused ? 'Resume simulation' : 'Pause simulation';
    }
  }
}

function updateStopButton() {
  const stopButton = document.getElementById('stopButton');
  if (stopButton) {
    stopButton.textContent = isStopped ? 'Simulation stopped' : 'Stop simulation';
    stopButton.disabled = isStopped;
  }
}

async function run() {
  await init();
  await initialize();
  
  let canvas = document.getElementById("sim-surface");
  resizeCanvas(canvas);
  
  // Initialize the parameters panel
  initParametersPanel();
  
  // Start the GPU-based simulation with integrated rendering
  // The start() function uses winit event loop and handles everything internally
  start();
  
  console.log("Fire simulation started with GPU rendering");
}

run();

// Stop button handler
document.getElementById("stopButton").addEventListener("click", () => {
  if (!isStopped) {
    SimulationController.stop();
    isStopped = true;
    isPaused = false;
    updateStopButton();
    updatePauseButton();
    console.log("Simulation stopped");
  }
});

// Pause button handler
document.getElementById("pauseButton").addEventListener("click", () => {
  if (isStopped) {
    // Resume from stopped state
    SimulationController.resume();
    isStopped = false;
    isPaused = false;
    updateStopButton();
    updatePauseButton();
    console.log("Simulation resumed from stop");
  } else {
    // Toggle pause
    SimulationController.toggle_pause();
    isPaused = !isPaused;
    updatePauseButton();
    console.log(isPaused ? "Simulation paused" : "Simulation resumed");
  }
});
