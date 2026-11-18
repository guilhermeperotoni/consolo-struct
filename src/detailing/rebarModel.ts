import type { Piece } from "../domain/model";
import {
  buildRebarGeometry,
  familyMark,
  type Bar,
  type RebarGeometry,
} from "./model";
import {
  cutPart,
  partPoints3,
  projectPart,
  type BarInstance,
  type PlacedPath,
  type Point3,
  type SectionCut,
  type ViewShape,
} from "./rebarPlacement";

/** Canonical reinforcement model of the ConsoloStruct (issue #63).
 *
 * `buildRebarGeometry` instantiates every detailed position once — mark,
 * function, diameter, steel, quantity, spacing, fabrication shape, anchorage
 * provisions, developed/cut lengths and the physical bars placed in 3D.
 * Elevation, sections A–A/B–B, plan, the 3D scene, the steel schedule and the
 * steel/concrete quantities are all derived from it by the pure functions
 * below; no renderer rebuilds bar coordinates on its own. */
export type RebarModel = RebarGeometry;
/** Anything carrying the placed bars: the geometry model or a full Detail. */
export type RebarSource = Pick<RebarGeometry, "piece" | "bars" | "arrangement">;

const cache = new Map<string, RebarModel>();
const CACHE_SIZE = 24;

/** Pure geometry model of one piece, memoized by its complete content.
 * Any edited input changes the key, so a stale model is never reused. It does
 * not run the ELU engine, except for the beam arrangement (anchored bends). */
export function rebarModel(p: Piece): RebarModel {
  const key = JSON.stringify(p);
  const hit = cache.get(key);
  if (hit) {
    cache.delete(key);
    cache.set(key, hit);
    return hit;
  }
  const model = buildRebarGeometry(p);
  cache.set(key, model);
  if (cache.size > CACHE_SIZE) cache.delete(cache.keys().next().value!);
  return model;
}

/** Family label used by the modelling views (selection, colours, 3D ids). */
export const viewFamily = (bar: Bar) =>
  bar.family === "stitch" || bar.family === "lower"
    ? "N2"
    : bar.family === "stirrup" || bar.family === "stirrupInner"
      ? "N3"
      : familyMark[bar.family];

type Placed = { bar: Bar; instance: BarInstance; part: PlacedPath };
function placedParts(src: RebarSource): Placed[] {
  return src.bars.flatMap((bar) =>
    bar.instances.flatMap((instance) =>
      instance.parts.map((part) => ({ bar, instance, part })),
    ),
  );
}

export type ViewItem = Placed & { shape: ViewShape };
/** Orthographic elevation (x, z): exact arcs for parts in vertical
 * longitudinal planes; horizontal runs and stirrups collapse to their extents. */
export function projectElevation(src: RebarSource): ViewItem[] {
  return placedParts(src).flatMap((item) => {
    const shape = projectPart(item.part, "elevation");
    return shape ? [{ ...item, shape }] : [];
  });
}
/** Orthographic plan (x, y), seen from the top face. */
export function projectPlan(src: RebarSource): ViewItem[] {
  return placedParts(src).flatMap((item) => {
    const shape = projectPart(item.part, "plan");
    return shape ? [{ ...item, shape }] : [];
  });
}

export type SectionItem = Placed & { cut: SectionCut };
/** Bars of the transverse plane x, in section coordinates (y, z).
 *
 * Only parts whose 3D path meets the plane are returned. With `pairWindow`,
 * stirrups whose own plane lies within that distance are projected onto the
 * section (the juxtaposed N3/N3G pair), and bars hosted by a stirrup (N4) are
 * cut in the plane of their host, as fabricated. */
export function intersectSectionX(
  src: RebarSource,
  x: number,
  { pairWindow = 0 }: { pairWindow?: number } = {},
): SectionItem[] {
  const items = placedParts(src);
  const window = pairWindow + 1e-6;
  const stations = (family: BarInstance["host"]) =>
    items
      .filter(
        ({ bar, part }) =>
          bar.family === family &&
          part.plane === "yz" &&
          Math.abs(part.at - x) <= window,
      )
      .map(({ part }) => part.at)
      .sort((a, b) => Math.abs(a - x) - Math.abs(b - x));
  return items.flatMap((item) => {
    const { part, instance } = item;
    if (part.plane === "yz") {
      if (Math.abs(part.at - x) > window) return [];
      return [
        {
          ...item,
          cut: { points: [], lines: [], inPlane: part.path, x: part.at },
        },
      ];
    }
    const station =
      pairWindow > 0 && instance.host ? (stations(instance.host)[0] ?? x) : x;
    const cut = cutPart(part, station);
    return cut.points.length || cut.lines.length ? [{ ...item, cut }] : [];
  });
}

export type ScenePath = Placed & { points: Point3[] };
/** 3D centre lines of every placed part, in construction order. */
export function scenePaths(src: RebarSource, divisions = 24): ScenePath[] {
  return placedParts(src)
    .sort((a, b) => a.instance.order - b.instance.order)
    .map((item) => ({ ...item, points: partPoints3(item.part, divisions) }));
}

/** Physical bars of every position against its declared quantity. */
export function quantityMismatches(src: RebarSource) {
  return src.bars
    .filter((bar) => bar.instances.length !== bar.quantity)
    .map((bar) => ({
      mark: bar.mark,
      declared: bar.quantity,
      placed: bar.instances.length,
    }));
}
