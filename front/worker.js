import init, { worker_entry } from "./pkg/firesim.js";

self.onmessage = event => {
  let initialized = init(...event.data);
  self.onmessage = async event => {
    await initialized;
    worker_entry(event.data);
  };
}
