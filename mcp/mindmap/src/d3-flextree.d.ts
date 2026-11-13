declare module "d3-flextree" {
  export interface FlexNode<T> {
    data: T;
    x: number;
    y: number;
    children: FlexNode<T>[] | null;
  }
  export interface FlexLayout<T> {
    (tree: FlexNode<T>): FlexNode<T>;
    hierarchy(data: T): FlexNode<T>;
    nodeSize(fn: (node: FlexNode<T>) => [number, number]): FlexLayout<T>;
    spacing(fn: number | ((a: FlexNode<T>, b: FlexNode<T>) => number)): FlexLayout<T>;
    children(fn: (data: T) => T[] | undefined | null): FlexLayout<T>;
  }
  export function flextree<T = unknown>(options?: {
    children?: (data: T) => T[] | undefined | null;
    nodeSize?: (node: FlexNode<T>) => [number, number];
    spacing?: number | ((a: FlexNode<T>, b: FlexNode<T>) => number);
  }): FlexLayout<T>;
}
