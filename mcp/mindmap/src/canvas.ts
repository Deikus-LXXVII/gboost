/** JSON Canvas 1.0 (https://jsoncanvas.org/spec/1.0/), the subset Bastion writes. */
export type CanvasSide = "top" | "right" | "bottom" | "left";
export type CanvasEnd = "none" | "arrow";

export interface CanvasTextNode {
  id: string;
  type: "text";
  x: number;
  y: number;
  width: number;
  height: number;
  color?: string;
  text: string;
}
export type CanvasNode = CanvasTextNode;

export interface CanvasEdge {
  id: string;
  fromNode: string;
  fromSide?: CanvasSide;
  fromEnd?: CanvasEnd;
  toNode: string;
  toSide?: CanvasSide;
  toEnd?: CanvasEnd;
  color?: string;
  label?: string;
}

export interface Canvas {
  nodes: CanvasNode[];
  edges: CanvasEdge[];
}
