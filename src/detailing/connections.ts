import type { Piece } from "../domain/model";
import { tieLayout } from "../domain/engine";
import { beamArrangementActive } from "./beam";

export function pinCenters(p: Piece): number[] {
  const s = p.detailing;
  const pitch =
    s.pinSpacing > 0 ? s.pinSpacing : p.design.bearingWidth / (s.pins + 1);
  return Array.from(
    { length: s.pins },
    (_, i) =>
      p.geometry.width / 2 + s.pinCenterOffset + (i - (s.pins - 1) / 2) * pitch,
  );
}

/** Physical connection geometry in mm, shared by detailing and elevation.
 * It does not run the design engine or add these bars to the resistance. */
export function connectionGeometry(p: Piece, layout = tieLayout(p)) {
  const { geometry: g, detailing: s, design: q } = p;
  const pinLength = s.pinEmbedment + s.pinProjection;
  const pins =
    s.pins > 0 && pinLength > 0
      ? {
          x: g.loadOffset,
          top: -s.pinProjection,
          bottom: s.pinEmbedment,
          length: pinLength,
          diameter: s.pinDiameter,
          quantity: s.pins,
          centers: pinCenters(p),
        }
      : null;
  const weldedLength = g.width - 2 * g.cover;
  const welded =
    !beamArrangementActive(p) && q.frontAnchor === "welded"
      ? layout.rows
          .filter((row) => row.count > 0)
          .map((row, layer) => ({
            x: g.length - g.cover - p.rebar.tieDiameter / 2,
            depth: row.y,
            layer,
            length: weldedLength,
            diameter: q.weldedBarDiameter,
          }))
      : [];
  return { pins, welded };
}
