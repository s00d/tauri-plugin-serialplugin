import { UnlistenFn } from '@tauri-apps/api/event';
export interface PortInfo {
    path: "Unknown" | string;
    manufacturer: "Unknown" | string;
    pid: "Unknown" | string;
    product: "Unknown" | string;
    serial_number: "Unknown" | string;
    type: "PCI" | string;
    vid: "Unknown" | string;
}
export interface InvokeResult {
    code: number;
    message: string;
}
export interface ReadDataResult {
    size: number;
    data: number[];
}
export interface SerialportOptions {
    path: string;
    baudRate: number;
    encoding?: string;
    dataBits?: 5 | 6 | 7 | 8;
    flowControl?: null | 'Software' | 'Hardware';
    parity?: null | 'Odd' | 'Even';
    stopBits?: 1 | 2;
    timeout?: number;
    size?: number;
    is_test?: boolean;
    [key: string]: any;
}
export interface Options {
    path?: string;
    baudRate?: number;
    dataBits: 5 | 6 | 7 | 8;
    flowControl: null | 'Software' | 'Hardware';
    parity: null | 'Odd' | 'Even';
    stopBits: 1 | 2;
    timeout: number;
    [key: string]: any;
}
export interface ReadOptions {
    timeout?: number;
    size?: number;
}
declare class SerialPort {
    isOpen: boolean;
    unListen?: UnlistenFn;
    encoding: string;
    options: Options;
    size: number;
    is_test: boolean;
    constructor(options: SerialportOptions);
    /**
     * @description: Get serial port list
     * @return {Promise<string[]>}
     */
    static available_ports(): Promise<{
        [key: string]: PortInfo;
    }>;
    /**
     * @description: force close
     * @param {string} path
     * @return {Promise<void>}
     */
    static forceClose(path: string): Promise<void>;
    /**
     * @description: close all serial ports
     * @return {Promise<void>}
     */
    static closeAll(): Promise<void>;
    /**
     * @description: Cancel serial port monitoring
     * @return {Promise<void>}
     */
    cancelListen(): Promise<void>;
    /**
     * @description: Cancel reading data
     * @return {Promise<void>}
     */
    cancelRead(): Promise<void>;
    /**
     * @description:
     * @param {object} options
     * @return {Promise<void>}
     */
    change(options: {
        path?: string;
        baudRate?: number;
    }): Promise<void>;
    /**
     * @description: close the serial port
     * @return {Promise<InvokeResult>}
     */
    close(): Promise<void>;
    disconnected(fn: (...args: any[]) => void): Promise<void>;
    /**
     * @description: Monitor serial port information
     * @param {function} fn
     * @param isDecode
     * @return {Promise<void>}
     */
    listen(fn: (...args: any[]) => void, isDecode?: boolean): Promise<void>;
    /**
     * @description: open serial port
     * @return {*}
     */
    open(): Promise<void>;
    /**
     * @description: Read serial port information
     * @param {ReadOptions} options read option { timeout, size }
     * @return {Promise<void>}
     */
    read(options?: ReadOptions): Promise<void>;
    /**
     * @description: Set serial port baud rate
     * @param {number} value
     * @return {Promise<void>}
     */
    setBaudRate(value: number): Promise<void>;
    /**
     * @description: Set the serial port path
     * @param {string} value
     * @return {Promise<void>}
     */
    setPath(value: string): Promise<void>;
    /**
     * @description: Serial port write data
     * @param {string} value
     * @return {Promise<number>}
     */
    write(value: string): Promise<number>;
    /**
     * @description: Write binary data to the serial port
     * @param {Uint8Array} value
     * @return {Promise<number>}
     */
    writeBinary(value: Uint8Array | number[]): Promise<number>;
}
export { SerialPort };
