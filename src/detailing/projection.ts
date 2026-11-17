import type { Piece } from "../domain/model";
import { tieLayout } from "../domain/engine";
import { beamArrangementActive, beamBarPositions } from "./beam";
import { connectionGeometry } from "./connections";
import {
  mandrelDiameter,
  roundedPath,
  samplePath,
  type Point,
} from "./geometry";
import type { Detail } from "./model";

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

export function elevationTiePaths(p: Piece, detail: Detail): Point[][] {
  const g = p.geometry,
    q = p.design,
    r = p.rebar,
    s = p.detailing;
  const beam = beamArrangementActive(p);
  if (beam) {
    return detail.bars
      .filter((b) => b.family === "tie")
      .map((b) => samplePath(b.path, 24));
  }
  const layout = tieLayout(p);
  const mainD = mandrelDiameter(r.tieDiameter, p.materials.fyk);
  const mainR = (mainD + r.tieDiameter) / 2;
  const front = g.length - g.cover - r.tieDiameter / 2;
  const back =
    q.supportAnchor === "hook"
      ? s.supportHookProjection
      : q.supportAnchorLength;
  return layout.rows
    .filter((row) => row.count > 0)
    .map((row) => {
      if (q.supportAnchor === "hook") {
        const path = roundedPath(
          [
            { x: front, y: row.y },
            { x: -back, y: row.y },
            { x: -back, y: row.y + mainR + s.supportHookTail },
          ],
          mainR,
        );
        return samplePath(path, 12);
      }
      return [
        { x: -back, y: row.y },
        { x: front, y: row.y },
      ];
    });
}

export function sectionBars(p: Piece, detail: Detail, sectionX: number) {
  const g = p.geometry,
    q = p.design,
    r = p.rebar;
  const beam = beamArrangementActive(p);
  const ties: { v: number; depth: number }[] = [];
  for (const b of detail.bars.filter((bar) => bar.family === "tie")) {
    if (q.frontAnchor === "loop" && !beam) {
      const inset = (g.width - b.width) / 2;
      for (const loc of b.locations) {
        ties.push({ v: inset, depth: loc.z });
        ties.push({ v: g.width - inset, depth: loc.z });
      }
    } else {
      for (const v of beamBarPositions(p, b.quantity, b.diameter))
        ties.push({ v, depth: b.locations[0].z });
    }
  }
  const stitches = detail.bars
    .filter((b) => b.family === "stitch" || b.family === "lower")
    .flatMap((b) =>
      b.locations
        .filter((loc) => loc.x >= sectionX - 1e-6)
        .map((loc) => ({ depth: loc.z, x: loc.x })),
    );
  const slope = (g.height - g.tipHeight) / g.length;
  const positives = detail.bars
    .filter((b) => b.family === "positive")
    .flatMap((b) => {
      const depth =
        g.height -
        slope * sectionX -
        (g.cover + r.stirrupDiameter + b.diameter / 2) *
          Math.sqrt(1 + slope ** 2);
      return beamBarPositions(p, b.quantity, b.diameter).map((v) => ({
        v,
        depth,
        diameter: b.diameter,
      }));
    });
  return { ties, stitches, positives };
}
