// Pure math for framing a 3D model in view, split out of Model3DViewer.tsx so it's
// unit-testable without a WebGL context — game meshes come in wildly different unit scales
// (a sword vs. a standing stone circle) and the camera needs to adapt every time.

export interface Framing {
  fitDistance: number;
  near: number;
  far: number;
}

export function computeFraming(size: { x: number; y: number; z: number }, fovDegrees: number): Framing {
  const maxDim = Math.max(size.x, size.y, size.z, 0.001);
  const fovRadians = (fovDegrees * Math.PI) / 360;
  const fitDistance = (maxDim / 2 / Math.tan(fovRadians)) * 1.6;
  return {
    fitDistance,
    near: maxDim / 100,
    far: maxDim * 100,
  };
}
