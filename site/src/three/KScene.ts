// The Kea mark in porcelain: the approved K, built from its own SVG geometry, assembled from its
// three pieces when the page opens, tilting to the pointer, and taken apart again as the page scrolls.
import * as THREE from "three";
import { RoomEnvironment } from "three/examples/jsm/environments/RoomEnvironment.js";

// The approved mark in its 512 box (brand logo v2): stem, upper arm, and the glazed lower arm.
const STEM =
  "M137.50 106.00 Q137.50 100.00 143.50 100.00 L201.50 100.00 Q207.50 100.00 207.50 106.00 L207.50 406.00 Q207.50 412.00 201.50 412.00 L143.50 412.00 Q137.50 412.00 137.50 406.00 Z";
const UPPER =
  "M218.50 206.50 L320.76 104.24 Q325.00 100.00 329.25 104.24 L370.26 145.25 Q374.50 149.50 370.26 153.74 L218.50 305.50 Z";
const LOWER =
  "M218.50 305.50 L320.76 407.76 Q325.00 412.00 329.25 407.76 L370.26 366.75 Q374.50 362.50 370.26 358.26 L218.50 206.50 Z";

function shapeFrom(d: string): THREE.Shape {
  const tokens = d.match(/[MLQZ]|-?\d+(?:\.\d+)?/g) ?? [];
  const shape = new THREE.Shape();
  let i = 0;
  const num = () => Number(tokens[i++]);
  // SVG y runs down; the scene's y runs up.
  const at = () => {
    const x = num();
    const y = num();
    return [x - 256, 256 - y] as const;
  };
  while (i < tokens.length) {
    const command = tokens[i++];
    if (command === "M") shape.moveTo(...at());
    else if (command === "L") shape.lineTo(...at());
    else if (command === "Q") {
      const [cx, cy] = at();
      const [x, y] = at();
      shape.quadraticCurveTo(cx, cy, x, y);
    } else if (command === "Z") shape.closePath();
  }
  return shape;
}

/** A faint, even unevenness, so the glaze reads as made rather than rendered. */
function glazeNoise(): THREE.CanvasTexture {
  const size = 256;
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = size;
  const context = canvas.getContext("2d")!;
  const image = context.createImageData(size, size);
  let seed = 7;
  const random = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);
  for (let p = 0; p < size * size; p++) {
    const v = 118 + random() * 20;
    image.data.set([v, v, v, 255], p * 4);
  }
  context.putImageData(image, 0, 0);
  const texture = new THREE.CanvasTexture(canvas);
  texture.wrapS = texture.wrapT = THREE.RepeatWrapping;
  texture.repeat.set(0.006, 0.006);
  return texture;
}

type Piece = {
  mesh: THREE.Mesh;
  home: THREE.Vector3;
  away: THREE.Vector3;
  turn: THREE.Euler;
  delay: number;
};

const easeOutExpo = (t: number) => (t >= 1 ? 1 : 1 - 2 ** (-10 * t));
const clamp01 = (t: number) => Math.min(1, Math.max(0, t));

/** What the page asks of the mark this frame. The scene eases toward it, so every change glides. */
export type KTarget = {
  /** Centre on screen, as fractions of the viewport. */
  x: number;
  y: number;
  /** Height as a fraction of the viewport height. */
  height: number;
  /** 0 is the whole K; 1 is every piece taken apart. */
  apart: number;
  /** The piece pulled toward the reader (0 stem, 1 arm, 2 glazed leg), or -1. */
  focus: number;
  /** Where a scanning light crosses the mark (0..1), or -1 for none. */
  sweep: number;
  /** Extra turn, in radians. */
  turn: number;
  /** Whether a pointer close by may part the pieces. */
  hover: boolean;
};

export type KSceneOptions = {
  target: () => KTarget;
  reducedMotion: boolean;
};

/** Frame-rate independent easing toward a target: the same glide at 60 or 120 fps. */
const damp = (current: number, target: number, rate: number, dt: number) =>
  current + (target - current) * (1 - Math.exp(-rate * dt));

export class KScene {
  private renderer: THREE.WebGLRenderer;
  private scene = new THREE.Scene();
  private camera = new THREE.PerspectiveCamera(30, 1, 0.1, 100);
  private mark = new THREE.Group();
  private tilt = new THREE.Group();
  private pieces: Piece[] = [];
  private pointer = new THREE.Vector2(0, 0);
  private smoothPointer = new THREE.Vector2(0, 0);
  private started = performance.now();
  private disposed = false;
  private hover = 0;
  private last = 0;
  private state = { x: 0.72, y: 0.45, height: 0.5, apart: 0, turn: 0, sweep: 0, light: 0, focus: [0, 0, 0] };
  private scanner = new THREE.PointLight(0xfff3e2, 0, 6, 1.6);
  private placed = false;

  constructor(
    canvas: HTMLCanvasElement,
    private options: KSceneOptions,
  ) {
    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, powerPreference: "high-performance" });
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;
    this.renderer.toneMapping = THREE.ACESFilmicToneMapping;
    this.renderer.toneMappingExposure = 0.92;
    this.renderer.setClearColor(0x000000, 0);

    const pmrem = new THREE.PMREMGenerator(this.renderer);
    this.scene.environment = pmrem.fromScene(new RoomEnvironment(), 0.04).texture;
    this.scene.environmentIntensity = 0.6;
    pmrem.dispose();

    // A warm key from high left gives each face its own value; a rim from behind right cuts the
    // edges out of the dark ground. The porcelain should read as fired clay, not plastic.
    const key = new THREE.DirectionalLight(0xfff1de, 2.6);
    key.position.set(-4, 5, 3.5);
    const rim = new THREE.DirectionalLight(0xffe2c4, 2.2);
    rim.position.set(4.5, 2, -3.5);
    const fill = new THREE.HemisphereLight(0xf6efe0, 0x1c200d, 0.3);
    this.scene.add(key, rim, fill);
    this.tilt.add(this.scanner);

    this.camera.position.set(0, 0, 5.2);
    this.buildMark();
    this.tilt.add(this.mark);
    this.scene.add(this.tilt);
    if (options.reducedMotion) this.started -= 10_000;

    window.addEventListener("pointermove", this.onPointer, { passive: true });
    window.addEventListener("resize", this.onResize);
    this.onResize();
    this.renderer.setAnimationLoop(this.frame);
  }

  private buildMark() {
    const noise = glazeNoise();
    const porcelain = new THREE.MeshPhysicalMaterial({
      color: 0xeae0ca,
      roughness: 0.38,
      clearcoat: 1,
      clearcoatRoughness: 0.16,
      sheen: 0.4,
      sheenRoughness: 0.6,
      sheenColor: new THREE.Color(0xfff4dc),
      bumpMap: noise,
      bumpScale: 0.6,
    });
    const glaze = new THREE.MeshPhysicalMaterial({
      // Darker than the brand swatch: lit and tone-mapped, it lands on the brand's vermilion.
      color: 0xc93f19,
      roughness: 0.2,
      clearcoat: 1,
      clearcoatRoughness: 0.06,
      bumpMap: noise,
      bumpScale: 0.35,
    });
    const extrude: THREE.ExtrudeGeometryOptions = {
      depth: 52,
      bevelEnabled: true,
      bevelThickness: 13,
      bevelSize: 10,
      bevelOffset: -10,
      bevelSegments: 12,
      curveSegments: 18,
    };
    const parts: [string, THREE.Material, number, THREE.Vector3, THREE.Euler, number][] = [
      [STEM, porcelain, 0, new THREE.Vector3(-150, 40, 120), new THREE.Euler(0.15, 0.9, -0.35), 0],
      [UPPER, porcelain, -4, new THREE.Vector3(150, 140, -90), new THREE.Euler(0.6, -0.7, 0.45), 140],
      [LOWER, glaze, 4, new THREE.Vector3(160, -150, 140), new THREE.Euler(-0.7, 0.5, -0.4), 280],
    ];
    for (const [d, material, z, away, turn, delay] of parts) {
      const geometry = new THREE.ExtrudeGeometry(shapeFrom(d), extrude);
      geometry.computeBoundingBox();
      const center = new THREE.Vector3();
      geometry.boundingBox!.getCenter(center);
      geometry.translate(-center.x, -center.y, -center.z);
      const mesh = new THREE.Mesh(geometry, material);
      const home = new THREE.Vector3(center.x, center.y, z);
      this.mark.add(mesh);
      this.pieces.push({ mesh, home, away: home.clone().add(away), turn, delay });
    }
    this.mark.scale.setScalar(1 / 256);
  }

  private onPointer = (event: PointerEvent) => {
    this.pointer.set(event.clientX / window.innerWidth - 0.5, event.clientY / window.innerHeight - 0.5);
  };

  private onResize = () => {
    const width = window.innerWidth;
    const height = window.innerHeight;
    // The canvas covers the whole window, so its pixel count is the cost: 1.5 keeps the edges clean at 60 fps.
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, width < 700 ? 1.25 : 1.5));
    this.renderer.setSize(width, height, false);
    this.camera.aspect = width / height;
    this.camera.updateProjectionMatrix();
  };

  // Development only: `?open=0.6` holds the opening at 0.6 s, so it can be checked frame by frame.
  private held = import.meta.env.DEV ? Number(new URLSearchParams(location.search).get("open") ?? NaN) : NaN;

  private frame = (time: number) => {
    if (this.disposed) return;
    const elapsed = Number.isFinite(this.held) ? this.held : (time - this.started) / 1000;
    const dt = this.last ? Math.min(0.1, (time - this.last) / 1000) : 1 / 60;
    this.last = time;
    const want = this.options.target();
    const st = this.state;
    const motion = !this.options.reducedMotion;

    // The first frame takes the target as it is; after that, everything glides.
    if (!this.placed) {
      Object.assign(st, { x: want.x, y: want.y, height: want.height });
      this.placed = true;
    }
    // With less motion asked for, the mark moves to each stage at once instead of gliding there.
    const glide = motion ? 5 : 1e6; // finite: a zero-length frame must not make NaN
    st.x = damp(st.x, want.x, glide, dt);
    st.y = damp(st.y, want.y, glide, dt);
    st.height = damp(st.height, want.height, glide, dt);
    st.apart = damp(st.apart, motion ? want.apart : 0, 4, dt);
    st.turn = damp(st.turn, motion ? want.turn : 0, 3, dt);
    // With less motion asked for, the mark stays whole and still: no scanning light, no piece pulled forward.
    st.light = damp(st.light, motion && want.sweep >= 0 ? 1 : 0, 4, dt);
    if (want.sweep >= 0) st.sweep = damp(st.sweep, want.sweep, 6, dt);
    st.focus = st.focus.map((f, i) => damp(f, motion && want.focus === i ? 1 : 0, 5, dt));

    // A pointer near the mark parts it a little: the pieces show they are pieces.
    const px = this.pointer.x + 0.5 - st.x;
    const py = (this.pointer.y + 0.5 - st.y) * (window.innerHeight / window.innerWidth);
    const near = motion && want.hover && Math.hypot(px, py) < st.height * 0.42 ? 0.16 : 0;
    this.hover = damp(this.hover, near, 4, dt);

    this.pieces.forEach((piece, i) => {
      const t = easeOutExpo(clamp01((elapsed * 1000 - 250 - piece.delay) / 1500));
      const spread = Math.max(1 - t, st.apart, this.hover);
      piece.mesh.position.lerpVectors(piece.home, piece.away, spread);
      // the focused piece comes toward the reader, drawn in toward the middle so it stays in frame,
      // and turns to show its face
      const pull = 1 - st.focus[i] * 0.35;
      piece.mesh.position.x *= pull;
      piece.mesh.position.y *= pull;
      piece.mesh.position.z += st.focus[i] * 110;
      piece.mesh.rotation.set(
        piece.turn.x * spread,
        piece.turn.y * spread - st.focus[i] * 0.35,
        piece.turn.z * spread,
      );
    });

    if (st.y < -0.9 || st.y > 1.9) return; // well off screen: nothing to draw
    const visibleHeight = 2 * this.camera.position.z * Math.tan(THREE.MathUtils.degToRad(this.camera.fov / 2));
    const visibleWidth = visibleHeight * this.camera.aspect;
    this.tilt.position.set((st.x - 0.5) * visibleWidth, (0.5 - st.y) * visibleHeight, 0);
    // The settle: a breath of scale as the pieces lock.
    const lock = clamp01((elapsed - 1.55) / 0.35);
    const breath = motion ? Math.sin(lock * Math.PI) * 0.018 : 0;
    this.tilt.scale.setScalar(((st.height * visibleHeight) / 1.22) * (1 + breath));

    // The scanning light crosses the mark in front of it, left to right.
    this.scanner.position.set(THREE.MathUtils.lerp(-0.9, 0.9, st.sweep), 0.1, 0.9);
    // Kept under the key light: brighter, and the glaze washes out to salmon.
    this.scanner.intensity = st.light * 4.5;

    // The pointer tilt is parallax: off when less motion is asked for.
    this.smoothPointer.x = damp(this.smoothPointer.x, motion ? this.pointer.x : 0, 3.5, dt);
    this.smoothPointer.y = damp(this.smoothPointer.y, motion ? this.pointer.y : 0, 3.5, dt);
    const intro = motion ? (1 - easeOutExpo(clamp01((elapsed - 0.1) / 2.2))) * -0.9 : 0;
    const sway = motion ? Math.sin(elapsed * 0.5) * 0.07 : 0;
    this.tilt.rotation.set(
      0.12 + this.smoothPointer.y * 0.28,
      -0.42 + intro + sway + st.turn + this.smoothPointer.x * 0.5,
      0,
    );
    this.mark.position.y = motion ? Math.sin(elapsed * 0.8) * 0.015 : 0;

    this.renderer.render(this.scene, this.camera);
  };

  dispose() {
    this.disposed = true;
    this.renderer.setAnimationLoop(null);
    window.removeEventListener("pointermove", this.onPointer);
    window.removeEventListener("resize", this.onResize);
    this.scene.traverse((object) => {
      if (object instanceof THREE.Mesh) {
        object.geometry.dispose();
        const materials = Array.isArray(object.material) ? object.material : [object.material];
        materials.forEach((material) => material.dispose());
      }
    });
    this.renderer.dispose();
  }
}
