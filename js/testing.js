export async function js_tests() {
  console.log("secure context? " + self.isSecureContext);
  console.log("cross-origin isolated? " + self.crossOriginIsolated);

  if (!navigator.gpu) {
    throw Error("WebGPU not supported.");
  }

  console.log("could get navigator.gpu");

  const adapter = await navigator.gpu.requestAdapter();
  if (!adapter) {
    throw Error("Couldn't request WebGPU adapter.");
  }
  console.log("got adapter!");

  const device = await adapter.requestDevice();

  console.log("got device!");
}
