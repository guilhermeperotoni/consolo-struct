import type { Piece } from "../domain/model";
import { beamBarPositions } from "./beam";
import { connectionGeometry } from "./connections";
import { sectionDepths } from "./transverse";
import {
  mandrelDiameter,
  roundedPath,
  samplePath,
  type Point,
} from "./geometry";
import type { Bar, Detail } from "./model";

/** Orthographic elevation of the connections missing from the modelling view.
 * N1/N2/N3/N4/P1 retain their existing physical renderers and cut planes. */
export type ElevationShape =
  | {
      kind: "line";
      family: "pin";
      mark: string;
      description: string;
      quantity: number;
      diameter: number;
      a: Point;
      b: Point;
    }
  | {
      kind: "end-on";
      family: "welded";
      mark: string;
      description: string;
      quantity: number;
      diameter: number;
      center: Point;
    };

export function elevationShapes(p: Piece): ElevationShape[] {
  const { pins, welded } = connectionGeometry(p);
  const shapes: ElevationShape[] = [];
  if (pins)
    // All transverse pin centres overlap in this orthographic view. Preserve
    // their physical count in the label rather than shifting them in pixels.
    shapes.push({
      kind: "line",
      family: "pin",
      mark: "PINO",
      description: "Pinos de ligação",
      quantity: pins.quantity,
      diameter: pins.diameter,
      a: { x: pins.x, y: pins.top },
      b: { x: pins.x, y: pins.bottom },
    });
  for (const [i, bar] of welded.entries()) {
    if (bar.length === 0) continue; // Same zero-length rejection as buildDetail.
    shapes.push({
      kind: "end-on",
      family: "welded",
      mark: welded.length === 1 ? "TS" : `TS-${i + 1}`,
      description: "Barra transversal soldada",
      quantity: 1,
      diameter: bar.diameter,
      center: { x: bar.x, y: bar.depth },
    });
  }
  return shapes;
}

/** The support hook is the extra out-of-plan length already recorded in Detail. */
function supportHook(bar: Bar, depth: number) {
  if (bar.hookExtra <= 0) return null;
  const radius = (mandrelDiameter(bar.diameter, bar.grade) + bar.diameter) / 2;
  const start = Math.min(...samplePath(bar.path, 24).map((pt) => pt.x));
  return roundedPath(
    [
      { x: start, y: depth },
      { x: start - radius, y: depth },
      { x: start - radius, y: depth + radius + bar.hookTail },
    ],
    radius,
  );
}

/** One elevation projection per detailed shape/depth; overlapping legs keep the bar's quantity. */
export function elevationTieBars(_p: Piece, detail: Detail) {
  return detail.bars
    .filter((b) => b.family === "tie")
    .flatMap((bar) => {
      if (detail.arrangement === "beam") return [{ bar, path: bar.path }];
      const xs = samplePath(bar.path, 24).map((pt) => pt.x);
      return [...new Set(bar.locations.map((loc) => loc.z))].map((depth) => {
        const hook = supportHook(bar, depth);
        const front = { x: Math.max(...xs), y: depth };
        const back = { x: Math.min(...xs), y: depth };
        const straight = roundedPath([front, back], 0);
        return {
          bar,
          path: hook
            ? {
                ...straight,
                segments: [...straight.segments, ...hook.segments],
                length: straight.length + hook.length,
                valid: straight.valid && hook.valid,
              }
            : straight,
        };
      });
    });
}

export function elevationTiePaths(p: Piece, detail: Detail): Point[][] {
  return elevationTieBars(p, detail).map(({ path }) => samplePath(path, 24));
}

export function sectionBars(p: Piece, detail: Detail, sectionX: number) {
  const g = p.geometry,
    q = p.design;
  const beam = detail.arrangement === "beam";
  const ties: { v: number; depth: number; diameter: number; mark: string }[] =
    [];
  for (const b of detail.bars.filter((bar) => bar.family === "tie")) {
    const add = (v: number, depth: number) => {
      if (
        !ties.some(
          (t) =>
            t.mark === b.mark &&
            Math.abs(t.v - v) < 1e-6 &&
            Math.abs(t.depth - depth) < 1e-6,
        )
      )
        ties.push({ v, depth, diameter: b.diameter, mark: b.mark });
    };
    if (beam) {
      for (const depth of sectionDepths(b.path, sectionX))
        for (const v of beamBarPositions(p, b.quantity, b.diameter))
          add(v, depth);
      continue;
    }
    if (q.frontAnchor === "loop") {
      const inset = (g.width - b.width) / 2;
      for (const loc of b.locations) {
        for (const v of sectionDepths(b.path, sectionX)) add(inset + v, loc.z);
        const hook = supportHook(b, loc.z);
        if (hook)
          for (const depth of sectionDepths(hook, sectionX)) {
            add(inset, depth);
            add(g.width - inset, depth);
          }
      }
    } else {
      const depth = b.locations[0].z;
      const hook = supportHook(b, depth);
      const depths = [
        ...sectionDepths(b.path, sectionX).map(() => depth),
        ...(hook ? sectionDepths(hook, sectionX) : []),
      ];
      for (const v of beamBarPositions(p, b.quantity, b.diameter))
        for (const z of depths) add(v, z);
    }
  }
  const stitches = detail.bars
    .filter((b) => b.family === "stitch" || b.family === "lower")
    .flatMap((b) =>
      b.locations.flatMap((loc) => {
        const positions = sectionDepths(
          b.path,
          sectionX + p.detailing.stitchEmbedment,
        ).map((v) => (g.width - b.height) / 2 + v);
        return positions.length
          ? [
              {
                depth: loc.z,
                x: loc.x,
                positions,
                diameter: b.diameter,
                mark: b.mark,
              },
            ]
          : [];
      }),
    );
  const positives = detail.bars
    .filter((b) => b.family === "positive")
    .flatMap((b) =>
      sectionDepths(b.path, sectionX).flatMap((depth) =>
        beamBarPositions(p, b.quantity, b.diameter).map((v) => ({
          v,
          depth,
          diameter: b.diameter,
          mark: b.mark,
        })),
      ),
    );
  // Connections are drawn as in-plane lines only when their longitudinal station is cut.
  const lines: {
    family: "pin" | "welded";
    mark: string;
    diameter: number;
    a: Point;
    b: Point;
  }[] = [];
  const connections = connectionGeometry(p);
  for (const bar of detail.bars) {
    if (
      bar.family === "pin" &&
      connections.pins &&
      Math.abs(connections.pins.x - sectionX) < 1e-6
    ) {
      for (const v of connections.pins.centers)
        lines.push({
          family: "pin",
          mark: bar.mark,
          diameter: bar.diameter,
          a: { x: v, y: connections.pins.top },
          b: { x: v, y: connections.pins.bottom },
        });
    }
    if (bar.family === "welded")
      for (const loc of bar.locations) {
        if (Math.abs(loc.x - sectionX) < 1e-6)
          lines.push({
            family: "welded",
            mark: bar.mark,
            diameter: bar.diameter,
            a: { x: g.cover, y: loc.z },
            b: { x: g.width - g.cover, y: loc.z },
          });
      }
  }
  return { ties, stitches, positives, lines };
}
