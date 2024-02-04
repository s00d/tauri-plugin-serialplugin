import { invoke } from '@tauri-apps/api/core';
import { Window } from '@tauri-apps/api/window';

const appWindow = new Window('serial-port');
let tester_ports = {};
let tester_listeners = {};
setInterval(() => {
    console.log('check test listeners');
    for (let path in tester_listeners) {
        console.log('send test to ' + path);
        tester_listeners[path]('random');
    }
}, 1000);
class SerialPort {
    constructor(options) {
        this.is_test = false;
        this.isOpen = false;
        this.encoding = options.encoding || 'utf-8';
        this.options = {
            path: options.path,
            baudRate: options.baudRate,
            dataBits: options.dataBits || 8,
            flowControl: options.flowControl || null,
            parity: options.parity || null,
            stopBits: options.stopBits || 2,
            timeout: options.timeout || 200,
        };
        this.size = options.size || 1024;
        this.is_test = options.is_test || false;
    }
    /**
     * @description: Get serial port list
     * @return {Promise<string[]>}
     */
    static async available_ports() {
        try {
            const result = await invoke('plugin:serialport|available_ports');
            for (const path in tester_ports) {
                result[path] = {
                    manufacturer: "tester",
                    pid: "tester",
                    product: "tester",
                    serial_number: "tester",
                    type: "USB",
                    vid: "tester",
                };
            }
            return Promise.resolve(result);
        }
        catch (error) {
            return Promise.reject(error);
        }
    }
    /**
     * @description: force close
     * @param {string} path
     * @return {Promise<void>}
     */
    static async forceClose(path) {
        if (tester_ports[path]) {
            delete tester_ports[path];
            return Promise.resolve();
        }
        return await invoke('plugin:serialport|force_close', {
            path,
        });
    }
    /**
     * @description: close all serial ports
     * @return {Promise<void>}
     */
    static async closeAll() {
        tester_ports = {};
        return await invoke('plugin:serialport|close_all');
    }
    /**
     * @description: Cancel serial port monitoring
     * @return {Promise<void>}
     */
    async cancelListen() {
        try {
            if (this.unListen) {
                this.unListen();
                this.unListen = undefined;
            }
            return;
        }
        catch (error) {
            return Promise.reject('Failed to cancel serial monitoring: ' + error);
        }
    }
    /**
     * @description: Cancel reading data
     * @return {Promise<void>}
     */
    async cancelRead() {
        if (this.is_test) {
            // todo check this
            return Promise.resolve();
        }
        try {
            return await invoke('plugin:serialport|cancel_read', {
                path: this.options.path,
            });
        }
        catch (error) {
            return Promise.reject(error);
        }
    }
    /**
     * @description:
     * @param {object} options
     * @return {Promise<void>}
     */
    async change(options) {
        try {
            let isOpened = false;
            if (this.isOpen) {
                isOpened = true;
                await this.close();
            }
            if (options.path) {
                this.options.path = options.path;
            }
            if (options.baudRate) {
                this.options.baudRate = options.baudRate;
            }
            if (isOpened) {
                await this.open();
            }
            return Promise.resolve();
        }
        catch (error) {
            return Promise.reject(error);
        }
    }
    /**
     * @description: close the serial port
     * @return {Promise<InvokeResult>}
     */
    async close() {
        try {
            if (!this.isOpen) {
                return;
            }
            await this.cancelRead();
            let res = undefined;
            if (!this.is_test) {
                res = await invoke('plugin:serialport|close', {
                    path: this.options.path,
                });
            }
            await this.cancelListen();
            this.isOpen = false;
            return res;
        }
        catch (error) {
            return Promise.reject(error);
        }
    }
    async disconnected(fn) {
        let sub_path = this.options.path?.toString().replace(/\.+/, '');
        let checkEvent = `plugin-serialport-disconnected-${sub_path}`;
        console.log('listen event: ' + checkEvent);
        let unListen = await appWindow.listen(checkEvent, () => {
            try {
                fn();
                unListen();
                unListen = undefined;
            }
            catch (error) {
                console.error(error);
            }
        });
    }
    /**
     * @description: Monitor serial port information
     * @param {function} fn
     * @param isDecode
     * @return {Promise<void>}
     */
    async listen(fn, isDecode = true) {
        try {
            await this.cancelListen();
            let sub_path = this.options.path?.toString().replace(/\.+/, '');
            let readEvent = `plugin-serialport-read-${sub_path}`;
            console.log('listen event: ' + readEvent);
            if (this.is_test) {
                console.log('add test event: ' + this.options.path, fn);
                tester_listeners[this.options.path] = fn;
                this.unListen = () => {
                    delete tester_listeners[this.options.path];
                };
                return Promise.resolve();
            }
            this.unListen = await appWindow.listen(readEvent, ({ payload }) => {
                try {
                    if (isDecode) {
                        const decoder = new TextDecoder(this.encoding);
                        const data = decoder.decode(new Uint8Array(payload.data));
                        fn(data);
                    }
                    else {
                        fn(new Uint8Array(payload.data));
                    }
                }
                catch (error) {
                    console.error(error);
                }
            });
            return;
        }
        catch (error) {
            return Promise.reject('Failed to monitor serial port data: ' + error);
        }
    }
    /**
     * @description: open serial port
     * @return {*}
     */
    async open() {
        try {
            if (!this.options.path) {
                return Promise.reject(`path Can not be empty!`);
            }
            if (!this.options.baudRate) {
                return Promise.reject(`baudRate Can not be empty!`);
            }
            if (this.isOpen) {
                return;
            }
            let res = undefined;
            if (this.is_test) {
                tester_ports[this.options.path] = this;
            }
            else {
                res = await invoke('plugin:serialport|open', {
                    path: this.options.path,
                    baudRate: this.options.baudRate,
                    dataBits: this.options.dataBits,
                    flowControl: this.options.flowControl,
                    parity: this.options.parity,
                    stopBits: this.options.stopBits,
                    timeout: this.options.timeout,
                });
            }
            this.isOpen = true;
            this.disconnected(() => {
                this.isOpen = false;
            }).catch(err => console.error(err));
            return Promise.resolve(res);
        }
        catch (error) {
            return Promise.reject(error);
        }
    }
    /**
     * @description: Read serial port information
     * @param {ReadOptions} options read option { timeout, size }
     * @return {Promise<void>}
     */
    async read(options) {
        try {
            if (this.is_test) {
                const resp = ''; // todo add reps
                if (tester_listeners[this.options.path])
                    tester_listeners[this.options.path](resp);
                return Promise.resolve();
            }
            return await invoke('plugin:serialport|read', {
                path: this.options.path,
                timeout: options?.timeout || this.options.timeout,
                size: options?.size || this.size,
            });
        }
        catch (error) {
            return Promise.reject(error);
        }
    }
    /**
     * @description: Set serial port baud rate
     * @param {number} value
     * @return {Promise<void>}
     */
    async setBaudRate(value) {
        try {
            let isOpened = false;
            if (this.isOpen) {
                isOpened = true;
                await this.close();
            }
            this.options.baudRate = value;
            if (isOpened) {
                await this.open();
            }
            return Promise.resolve();
        }
        catch (error) {
            return Promise.reject(error);
        }
    }
    /**
     * @description: Set the serial port path
     * @param {string} value
     * @return {Promise<void>}
     */
    async setPath(value) {
        try {
            let isOpened = false;
            if (this.isOpen) {
                isOpened = true;
                await this.close();
            }
            this.options.path = value;
            if (isOpened) {
                await this.open();
            }
            return Promise.resolve();
        }
        catch (error) {
            return Promise.reject(error);
        }
    }
    /**
     * @description: Serial port write data
     * @param {string} value
     * @return {Promise<number>}
     */
    async write(value) {
        try {
            if (!this.isOpen) {
                return Promise.reject(`serial port ${this.options.path} not opened!`);
            }
            if (this.is_test) {
                return Promise.resolve(2); // todo add resp
            }
            return await invoke('plugin:serialport|write', {
                value,
                path: this.options.path,
            });
        }
        catch (error) {
            return Promise.reject(error);
        }
    }
    /**
     * @description: Write binary data to the serial port
     * @param {Uint8Array} value
     * @return {Promise<number>}
     */
    async writeBinary(value) {
        try {
            if (!this.isOpen) {
                return Promise.reject(`serial port ${this.options.path} not opened!`);
            }
            if (value instanceof Uint8Array || value instanceof Array) {
                if (this.is_test) {
                    return Promise.resolve(2); // todo add resp
                }
                return await invoke('plugin:serialport|write_binary', {
                    value: Array.from(value),
                    path: this.options.path,
                });
            }
            else {
                return Promise.reject('value Argument type error! Expected type: string, Uint8Array, number[]');
            }
        }
        catch (error) {
            return Promise.reject(error);
        }
    }
}

export { SerialPort };
