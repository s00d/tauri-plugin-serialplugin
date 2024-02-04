import { Resource } from '../core';
export type ItemKind = 'MenuItem' | 'Predefined' | 'Check' | 'Icon' | 'Submenu' | 'Menu';
export declare function newMenu(kind: ItemKind, opts?: unknown): Promise<[number, string]>;
export declare class MenuItemBase extends Resource {
    #private;
    /** The id of this item. */
    get id(): string;
    /** @ignore */
    get kind(): string;
    /** @ignore */
    protected constructor(rid: number, id: string, kind: ItemKind);
}
