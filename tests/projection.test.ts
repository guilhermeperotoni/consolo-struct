import { describe, expect, it, vi } from "vitest";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import * as engine from "../src/domain/engine";
import { decodeProject, exampleProject } from "../src/domain/model";
import { connectionGeometry, pinCenters } from "../src/detailing/connections";
import { buildDetail } from "../src/detailing/model";
import { elevationShapes } from "../src/detailing/projection";
import Drawing from "../src/components/Drawing";
import { shortFixture } from "./fixtures";

function fixture() {
  const p = shortFixture();
  p.geometry.tipHeight = 450;
  p.detailing.pins = 3;
  p.design.frontAnchor = "welded";
  p.design.tieLayers = 2;
  p.rebar.tieCount = 6;
  return p;
}

function render(p = fixture(), section = false, rebar = true) {
  return renderToStaticMarkup(
    createElement(Drawing, {
      piece: p,
      section,
      rebar,
      dimensions: true,
      grid: false,
      zoom: 1,
      pan: { x: 0, y: 0 },
      forces: true,
      selectedBar: "N4",
      onDimension: () => {},
      onBar: () => {},
    }),
  );
}

describe("Projeções de ligações integradas a N3/N4 (#39)", () => {
  it("não acrescenta família à peça sem pinos e com alça", () => {
    expect(elevationShapes(shortFixture())).toEqual([]);
  });

  it.each([1, 2, 5, 20])(
    "projeta %s pinos sobrepostos, conservando quantidade e percurso físico",
    (quantity) => {
      const p = fixture();
      p.detailing.pins = quantity;
      p.detailing.pinSpacing = 45;
      p.detailing.pinCenterOffset = 12;
      p.detailing.pinEmbedment = 210;
      p.detailing.pinProjection = 90;
      const connection = connectionGeometry(p).pins!;
      expect(connection.centers).toEqual(pinCenters(p));
      expect(connection.centers).toHaveLength(quantity);
      expect(connection.centers[0]).toBe(212 - ((quantity - 1) * 45) / 2);
      const [shape] = elevationShapes(p).filter((s) => s.family === "pin");
      expect(shape).toMatchObject({
        kind: "line",
        quantity,
        a: { x: 300, y: -90 },
        b: { x: 300, y: 210 },
      });
      const bar = buildDetail(p).bars.find((b) => b.family === "pin")!;
      expect(bar.quantity).toBe(shape.quantity);
      expect(bar.exactLength).toBe(connection.bottom - connection.top);
      expect(bar.locations).toEqual([{ x: connection.x, z: 0 }]);
    },
  );

  it.each([1, 2, 3, 4])(
    "projeta a barra soldada nas %s camadas efetivamente ocupadas",
    (layers) => {
      const p = fixture();
      p.design.tieLayers = layers;
      p.rebar.tieCount = 8;
      p.design.weldedBarDiameter = 25;
      const bars = buildDetail(p).bars.filter((b) => b.family === "welded");
      const shapes = elevationShapes(p).filter((s) => s.kind === "end-on");
      expect(shapes).toHaveLength(layers);
      expect(shapes.map((s) => s.mark)).toEqual(bars.map((b) => b.mark));
      shapes.forEach((shape, i) => {
        expect(shape.center).toEqual({
          x: bars[i].locations[0].x,
          y: bars[i].locations[0].z,
        });
        expect(shape.diameter).toBe(bars[i].diameter);
        expect(shape.quantity).toBe(bars[i].quantity);
      });
    },
  );

  it("não cria barra soldada em camada vazia nem pino sem comprimento", () => {
    const p = fixture();
    p.design.tieLayers = 4;
    p.rebar.tieCount = 2;
    p.detailing.pinEmbedment = p.detailing.pinProjection = 0;
    const shapes = elevationShapes(p);
    expect(shapes.filter((s) => s.family === "pin")).toHaveLength(0);
    expect(shapes.filter((s) => s.family === "welded")).toHaveLength(2);
    expect(buildDetail(p).issues.some((i) => i.code === "shape-pin")).toBe(
      true,
    );
  });

  it("suprime a soldada no modo viga, mantendo pinos e a positiva independente", () => {
    const p = fixture();
    p.detailing.barArrangement = "beam";
    expect(elevationShapes(p).map((s) => s.family)).toEqual(["pin"]);
    expect(buildDetail(p).bars.some((b) => b.family === "positive")).toBe(true);
    const html = render(p);
    expect(html).toContain('data-testid="beam-positive"');
    expect(html).toContain('data-family="pin"');
    expect(html).not.toContain('data-family="welded"');
    p.design.excessConcrete = "remove";
    expect(elevationShapes(p).some((s) => s.family === "welded")).toBe(true);
  });

  it.each([2, 4] as const)(
    "preserva N2/N3/N4, a seção e d’ com %s ramos; não duplica o porta-estribos",
    (branches) => {
      const p = fixture();
      p.rebar.stirrupBranches = branches;
      // Four N1 bars per layer define the physical inner stirrup.
      p.rebar.tieCount = 8;
      const html = render(p);
      expect(html).toContain('data-quantity="3"');
      expect(html).toContain("PINO · 3 × Ø16,0 mm");
      expect(html.match(/data-transverse-position=/g)).toHaveLength(branches);
      expect(html).not.toContain("steel-mounting");
      const section = render(p, true);
      expect(section).toContain("steel-N2");
      expect(section).toContain("steel-N3");
      expect(section).toContain("steel-N4");
      expect(section).toContain("d’ =");
      expect(section).not.toContain("data-family=");
      expect(render(p, false, false)).not.toContain("detail-families");
    },
  );

  it("não calcula ELU nem altera entradas ao projetar a elevação", () => {
    const p = fixture(),
      before = JSON.stringify(p);
    const calculate = vi.spyOn(engine, "calculate");
    try {
      elevationShapes(p);
      expect(calculate).not.toHaveBeenCalled();
      expect(JSON.stringify(p)).toBe(before);
    } finally {
      calculate.mockRestore();
    }
  });

  it("reabre no esquema 5 sem restaurar mounting e conserva as projeções", () => {
    const p = fixture();
    const raw = JSON.stringify({ ...exampleProject(), pieces: [p] });
    const reopened = decodeProject(raw).pieces[0];
    expect(reopened.detailing).not.toHaveProperty("mounting");
    expect(elevationShapes(reopened)).toEqual(elevationShapes(p));
    expect(buildDetail(reopened).bars).toEqual(buildDetail(p).bars);
  });
});
