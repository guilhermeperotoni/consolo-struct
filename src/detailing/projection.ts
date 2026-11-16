import type { Piece } from "../domain/model";
import { connectionGeometry } from "./connections";
import type { Point } from "./geometry";

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
