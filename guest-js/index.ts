import {invoke} from "@tauri-apps/api/core";
import {listen, UnlistenFn} from '@tauri-apps/api/event';
import {AutoReconnectManager} from './auto-reconnect-manager';
import {ListenerManager} from './listener-manager';

// All type definitions for the serial plugin

export interface PortInfo {
  path: "Unknown" | string;
  manufacturer: "Unknown" | string;
  pid: "Unknown" | string;
  product: "Unknown" | string;
  serial_number: "Unknown" | string;
  type: "PCI" | string;
  vid: "Unknown" | string;
}

export interface ReadDataResult {
  size: number;
  data: number[];
}

export interface SerialportOptions {
  path: string;
  baudRate: number;
  encoding?: string;
  dataBits?: DataBits;
  flowControl?: FlowControl;
  parity?: Parity;
  stopBits?: StopBits;
  timeout?: number;
  size?: number;
  [key: string]: any;
}

export interface Options {
  path?: string;
  baudRate?: number;
  dataBits: DataBits;
  flowControl: FlowControl;
  parity: Parity;
  stopBits: StopBits;
  size?: number;
  timeout: number;
  [key: string]: any;
}

export interface ReadOptions {
  timeout?: number;
  size?: number;
}

export enum DataBits {
  Five = "Five",
  Six = "Six",
  Seven = "Seven",
  Eight = "Eight"
}

export enum FlowControl {
  None = "None",
  Software = "Software",
  Hardware = "Hardware"
}

export enum Parity {
  None = "None",
  Odd = "Odd",
  Even = "Even"
}

export enum StopBits {
  One = "One",
  Two = "Two"
}

export enum ClearBuffer {
  Input = "Input",
  Output = "Output",
  All = "All"
}

// Additional type utilities
export type SerialPortConfig = {
  path: string;
  baudRate: number;
  dataBits?: DataBits;
  flowControl?: FlowControl;
  parity?: Parity;
  stopBits?: StopBits;
  timeout?: number;
  size?: number;
  encoding?: string;
};

// Utility types for common operations
export type BaudRate =
    | 110 | 300 | 600 | 1200 | 2400 | 4800 | 9600
    | 14400 | 19200 | 38400 | 57600 | 115200
    | 230400 | 460800 | 921600;

class SerialPort {
  isOpen: boolean;
  encoding: string;
  options: Options;
  size: number;
  private listeners: ListenerManager = new ListenerManager();
  private autoReconnectManager: AutoReconnectManager = new AutoReconnectManager();

  constructor(options: SerialportOptions) {
    this.isOpen = false;
    this.encoding = options.encoding || 'utf-8';
    this.options = {
      path: options.path,
      baudRate: options.baudRate,
      dataBits: options.dataBits || DataBits.Eight,
      flowControl: options.flowControl || FlowControl.None,
      parity: options.parity || Parity.None,
      stopBits: options.stopBits || StopBits.One,
      size: options.size || 1024,
      timeout: options.timeout || 200,
    };
    this.size = options.size || 1024;
  }

  /**
   * @description Lists all available serial ports
   * @returns {Promise<{ [key: string]: PortInfo }>} A promise that resolves to a map of port names to port information
   */
  static async available_ports(): Promise<{ [key: string]: PortInfo }> {
    try {
      const result = await invoke<{ [key: string]: PortInfo }>('plugin:serialplugin|available_ports');
      return Promise.resolve(result)
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Lists all available serial ports using platform-specific commands
   * @returns {Promise<{ [key: string]: PortInfo }>} A promise that resolves to a map of port names to port information
   */
  static async available_ports_direct(): Promise<{ [key: string]: PortInfo }> {
    try {
      const result = await invoke<{ [key: string]: PortInfo }>('plugin:serialplugin|available_ports_direct');
      return Promise.resolve(result)
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Lists all managed serial ports (ports that are currently open and managed by the application).
   * @returns {Promise<string[]>} A promise that resolves to an array of port paths (names).
   */
  static async managed_ports(): Promise<string[]> {
    try {
      const result = await invoke<string[]>('plugin:serialplugin|managed_ports');
      return Promise.resolve(result);
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Forcefully closes a specific serial port
   * @param {string} path The path of the serial port to close
   * @returns {Promise<void>} A promise that resolves when the port is closed
   */
  static async forceClose(path: string): Promise<void> {
    return await invoke<void>('plugin:serialplugin|force_close', { path });
  }

  /**
   * @description Closes all open serial ports
   * @returns {Promise<void>} A promise that resolves when all ports are closed
   */
  static async closeAll(): Promise<void> {
    return await invoke<void>('plugin:serialplugin|close_all');
  }

  /**
   * @description Cancels listening for serial port data (does not affect disconnect listeners)
   * @returns {Promise<void>} A promise that resolves when listening is cancelled
   */
  async cancelListen(): Promise<void> {
    try {
      // Cancel only data listeners - disconnect listeners remain active
      const dataListeners = this.listeners.filterByType('data');
      for (const [id, listener] of dataListeners) {
        try {
          if (typeof listener.unlisten === 'function') {
            listener.unlisten();
          }
        } catch (error) {
          console.warn(`Error unlistening data listener ${id}:`, error);
        } finally {
          this.listeners.delete(id);
        }
      }
      return;
    } catch (error) {
      return Promise.reject('Failed to cancel serial monitoring: ' + error);
    }
  }

  /**
   * @description Cancels all listeners (both data and disconnect listeners)
   * @returns {Promise<void>} A promise that resolves when all listeners are cancelled
   */
  async cancelAllListeners(): Promise<void> {
    try {
      const allListeners = this.listeners.all();
      for (const [id, listener] of allListeners) {
        try {
          if (typeof listener.unlisten === 'function') {
            listener.unlisten();
          }
        } catch (error) {
          console.warn(`Error unlistening listener ${id}:`, error);
        } finally {
          this.listeners.delete(id);
        }
      }
      return;
    } catch (error) {
      return Promise.reject('Failed to cancel all listeners: ' + error);
    }
  }

  /**
   * @description Gets information about active listeners (for debugging)
   * @returns {Object} Information about active listeners
   */
  getListenersInfo(): { total: number; data: number; disconnect: number; ids: string[] } {
    return this.listeners.getInfo();
  }

  /**
   * @description Cancels reading data from the serial port
   * @returns {Promise<void>} A promise that resolves when reading is cancelled
   */
  private async cancelRead(): Promise<void> {
    try {
      await invoke<void>('plugin:serialplugin|cancel_read', {
        path: this.options.path,
      });
    } catch (error) {
      return Promise.reject(error instanceof Error ? error : new Error(String(error)));
    }
  }

  /**
   * @description Changes the serial port configuration
   * @param {object} options Configuration options
   * @param {string} [options.path] New port path
   * @param {number} [options.baudRate] New baud rate
   * @returns {Promise<void>} A promise that resolves when configuration is changed
   */
  async change(options: { path?: string; baudRate?: number }): Promise<void> {
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
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Closes the currently open serial port
   * @returns {Promise<void>} A promise that resolves when the port is closed
   */
  async close(): Promise<void> {
    try {
      if (!this.isOpen) {
        return;
      }

      // Stop auto-reconnect temporarily to prevent conflicts
      const wasAutoReconnectEnabled = this.autoReconnectManager.isEnabled();
      if (wasAutoReconnectEnabled) {
        await this.autoReconnectManager.stop();
      }

      // First we cancel the reading
      try {
        await this.cancelRead();
      } catch (cancelReadError) {
        console.warn('Error during cancelRead:', cancelReadError);
      }

      // Closing the port
      let res = undefined;
      try {
        res = await invoke<void>('plugin:serialplugin|close', {
          path: this.options.path,
        });
      } catch (closeError) {
        console.warn('Error during port close:', closeError);
      }

      // Cancel all listeners
      try {
        await this.cancelAllListeners();
      } catch (cancelListenError) {
        console.warn('Error during cancelAllListeners:', cancelListenError);
      }

      this.isOpen = false;

      return res;
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Sets up a listener for port disconnection events
   * @param {Function} fn Callback function to handle disconnection
   * @returns {Promise<void>} A promise that resolves when the listener is set up
   */
  async disconnected(fn: (...args: any[]) => void): Promise<void> {
    let sub_path = this.options.path?.toString().replaceAll(".", "-").replaceAll("/", "-")
    let checkEvent = `plugin-serialplugin-disconnected-${sub_path}`;
    console.log('listen event: ' + checkEvent)

    const unListenResult = await listen<ReadDataResult>(
        checkEvent,
        () => {
          try {
            fn();
          } catch (error) {
            console.error(error);
          }
        },
    );

    if (typeof unListenResult === 'function') {
      this.listeners.add('disconnect', unListenResult);
    } else {
      console.warn('disconnected() did not return a valid unlisten function');
    }
  }

  /**
   * @description Enables auto-reconnect functionality
   * @param {Object} options Auto-reconnect configuration options
   * @param {number} [options.interval=5000] Reconnection interval in milliseconds
   * @param {number | null} [options.maxAttempts=10] Maximum number of reconnection attempts (null for infinite)
   * @param {Function} [options.onReconnect] Callback function called on each reconnection attempt
   * @returns {Promise<void>} A promise that resolves when auto-reconnect is enabled
   */
  async enableAutoReconnect(options: {
    interval?: number;
    maxAttempts?: number | null;
    onReconnect?: (success: boolean, attempt: number) => void;
  } = {}): Promise<void> {
    try {
      await this.autoReconnectManager.enable({
        ...options,
        reconnectFunction: async (): Promise<boolean> => {
          if (this.isOpen) {
            return true;
          }
          try {
            await this.open();
            return true;
          } catch (error) {
            return false;
          }
        }
      });

      // Set up disconnect listener that triggers auto-reconnect
      await this.disconnected(async () => {
        this.isOpen = false;
        if (this.autoReconnectManager.isEnabled()) {
          await this.autoReconnectManager.start();
        }
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Disables auto-reconnect functionality
   * @returns {Promise<void>} A promise that resolves when auto-reconnect is disabled
   */
  async disableAutoReconnect(): Promise<void> {
    try {
      await this.autoReconnectManager.disable();
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Gets auto-reconnect status and configuration
   * @returns {Object} Auto-reconnect information
   */
  getAutoReconnectInfo(): {
    enabled: boolean;
    interval: number;
    maxAttempts: number | null;
    currentAttempts: number;
    hasCallback: boolean;
  } {
    return {
      enabled: this.autoReconnectManager.isEnabled(),
      interval: this.autoReconnectManager.getInterval(),
      maxAttempts: this.autoReconnectManager.getMaxAttempts(),
      currentAttempts: this.autoReconnectManager.getCurrentAttempts(),
      hasCallback: this.autoReconnectManager.hasCallback(),
    };
  }

  /**
   * @description Manually triggers a reconnection attempt
   * @returns {Promise<boolean>} A promise that resolves to true if reconnection was successful
   */
  async manualReconnect(): Promise<boolean> {
    try {
      if (this.isOpen) {
        console.log('Port is already open, no need to reconnect');
        return true;
      }

      console.log('Manual reconnection attempt...');
      await this.open();
      console.log('Manual reconnection successful');
      return true;
    } catch (error) {
      console.error('Manual reconnection failed:', error);
      return false;
    }
  }

  /**
   * @description Monitors serial port data
   * @param {Function} fn Callback function to handle received data
   * @param {boolean} [isDecode=true] Whether to decode the received data
   * @returns {Promise<UnlistenFn>} A promise that resolves to an unlisten function
   */
  async listen(fn: (...args: any[]) => void, isDecode: boolean = true): Promise<UnlistenFn> {
    try {
      if (!this.isOpen) {
        return Promise.reject('Port is not open');
      }

      let sub_path = this.options.path?.toString().replaceAll(".", "-").replaceAll("/", "-")
      let readEvent = `plugin-serialplugin-read-${sub_path}`;
      console.log('listen event: ' + readEvent)

      try {
        const unListenResult = await listen<ReadDataResult>(
            readEvent,
            ({ payload }) => {
              try {
                if (isDecode) {
                  const uint8Array = new Uint8Array(payload.data);
                  try {
                    const decoder = new TextDecoder(this.encoding);
                    const textData = decoder.decode(uint8Array);
                    fn(textData);
                  } catch (error) {
                    console.error('Error converting to text with configured encoding:', error);
                    try {
                      const fallbackDecoder = new TextDecoder('utf-8');
                      const textData = fallbackDecoder.decode(uint8Array);
                      fn(textData);
                    } catch (fallbackError) {
                      console.error('Fallback decoding also failed:', fallbackError);
                      fn(String.fromCharCode(...uint8Array));
                    }
                  }
                } else {
                  fn(new Uint8Array(payload.data));
                }
              } catch (error) {
                console.error(error);
              }
            },
        );

        if (typeof unListenResult === 'function') {
          return this.listeners.add('data', unListenResult);
        } else {
          console.warn('listen() did not return a valid unlisten function');
          return Promise.reject('Failed to get unlisten function');
        }
      } catch (listenError) {
        console.error('Error setting up listener:', listenError);
        throw listenError;
      }
    } catch (error) {
      return Promise.reject('Failed to monitor serial port data: ' + error);
    }
  }

  /**
   * @description Opens the serial port with current settings
   * @returns {Promise<void>} A promise that resolves when the port is opened
   */
  async open(): Promise<void> {
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

      const res = await invoke<void>('plugin:serialplugin|open', {
        path: this.options.path,
        baudRate: this.options.baudRate,
        dataBits: this.options.dataBits,
        flowControl: this.options.flowControl,
        parity: this.options.parity,
        stopBits: this.options.stopBits,
        timeout: this.options.timeout,
      });

      this.isOpen = true;

      this.disconnected(() => {
        this.isOpen = false;
      }).catch(err => console.error(err))
      return Promise.resolve(res);
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * Starts listening for data on the serial port
   * The port will continuously monitor for incoming data and emit events
   * @returns {Promise<void>} A promise that resolves when listening starts
   * @throws {Error} If starting listener fails or port is not open
   * @example
   * const port = new SerialPort({ path: '/dev/ttyUSB0' });
   * await port.startListening();
   * // Listen for data events
   * port.listen((data) => {
   *   console.log('listen', data)
   *   receivedData += data;
   * });
   */
  async startListening(): Promise<void> {
    try {
      await invoke<string>('plugin:serialplugin|start_listening', {
        path: this.options.path,
        size: this.options.size,
        timeout: this.options.timeout,
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * Stops listening for data on the serial port
   * Cleans up event listeners and monitoring threads
   * @returns {Promise<void>} A promise that resolves when listening stops
   * @throws {Error} If stopping listener fails or port is not open
   * @example
   * await port.stopListening();
   */
  async stopListening(): Promise<void> {
    try {
      await invoke<string>('plugin:serialplugin|stop_listening', {
        path: this.options.path,
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Reads data from the serial port
   * @param {ReadOptions} [options] Read options
   * @returns {Promise<void>} A promise that resolves when data is read
   */
  async read(options?: ReadOptions): Promise<string> {
    try {
      if (!this.isOpen) {
        return Promise.reject('Port is not open');
      }

      return await invoke<string>('plugin:serialplugin|read', {
        path: this.options.path,
        timeout: options?.timeout || this.options.timeout,
        size: options?.size || this.size,
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Reads binary data from the serial port
   * @param {ReadOptions} [options] Read options
   * @returns {Promise<Uint8Array>} A promise that resolves with binary data
   */
  async readBinary(options?: ReadOptions): Promise<Uint8Array> {
    try {
      const result = await invoke<number[]>('plugin:serialplugin|read_binary', {
        path: this.options.path,
        timeout: options?.timeout || this.options.timeout,
        size: options?.size || this.size,
      });

      return new Uint8Array(result);
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Sets the baud rate of the serial port
   * @param {number} value The new baud rate
   * @returns {Promise<void>} A promise that resolves when baud rate is set
   */
  async setBaudRate(value: number | BaudRate): Promise<void> {
    try {
      return await invoke<void>('plugin:serialplugin|set_baud_rate', {
        path: this.options.path,
        baudRate: value
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Sets the data bits configuration
   * @param {DataBits} value The new data bits setting
   * @returns {Promise<void>} A promise that resolves when data bits are set
   */
  async setDataBits(value: DataBits): Promise<void> {
    try {
      return await invoke<void>('plugin:serialplugin|set_data_bits', {
        path: this.options.path,
        dataBits: value
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Sets the flow control mode
   * @param {FlowControl} value The new flow control setting
   * @returns {Promise<void>} A promise that resolves when flow control is set
   */
  async setFlowControl(value: FlowControl): Promise<void> {
    try {
      return await invoke<void>('plugin:serialplugin|set_flow_control', {
        path: this.options.path,
        flowControl: value
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Sets the parity checking mode
   * @param {Parity} value The new parity setting
   * @returns {Promise<void>} A promise that resolves when parity is set
   */
  async setParity(value: Parity): Promise<void> {
    try {
      return await invoke<void>('plugin:serialplugin|set_parity', {
        path: this.options.path,
        parity: value
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Sets the number of stop bits
   * @param {StopBits} value The new stop bits setting
   * @returns {Promise<void>} A promise that resolves when stop bits are set
   */
  async setStopBits(value: StopBits): Promise<void> {
    try {
      return await invoke<void>('plugin:serialplugin|set_stop_bits', {
        path: this.options.path,
        stopBits: value
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Sets the timeout duration
   * @param {number} value The new timeout in milliseconds
   * @returns {Promise<void>} A promise that resolves when timeout is set
   */
  async setTimeout(value: number): Promise<void> {
    try {
      return await invoke<void>('plugin:serialplugin|set_timeout', {
        path: this.options.path,
        timeout: value
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Sets the RTS (Request To Send) control signal
   * @param {boolean} value The signal level to set
   * @returns {Promise<void>} A promise that resolves when RTS is set
   */
  async setRequestToSend(value: boolean): Promise<void> {
    try {
      return await invoke<void>('plugin:serialplugin|write_request_to_send', {
        path: this.options.path,
        level: value
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Sets the DTR (Data Terminal Ready) control signal
   * @param {boolean} value The signal level to set
   * @returns {Promise<void>} A promise that resolves when DTR is set
   */
  async setDataTerminalReady(value: boolean): Promise<void> {
    try {
      return await invoke<void>('plugin:serialplugin|write_data_terminal_ready', {
        path: this.options.path,
        level: value
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Writes the RTS (Request To Send) control signal
   * @param {boolean} level The signal level to set
   * @returns {Promise<void>} A promise that resolves when RTS is set
   */
  async writeRequestToSend(level: boolean): Promise<void> {
    try {
      return await invoke<void>('plugin:serialplugin|write_request_to_send', {
        path: this.options.path,
        level: level
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Writes the DTR (Data Terminal Ready) control signal
   * @param {boolean} level The signal level to set
   * @returns {Promise<void>} A promise that resolves when DTR is set
   */
  async writeDataTerminalReady(level: boolean): Promise<void> {
    try {
      return await invoke<void>('plugin:serialplugin|write_data_terminal_ready', {
        path: this.options.path,
        level: level
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Reads the CTS (Clear To Send) control signal state
   * @returns {Promise<boolean>} A promise that resolves to the CTS state
   */
  async readClearToSend(): Promise<boolean> {
    try {
      return await invoke<boolean>('plugin:serialplugin|read_clear_to_send', {
        path: this.options.path
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Reads the DSR (Data Set Ready) control signal state
   * @returns {Promise<boolean>} A promise that resolves to the DSR state
   */
  async readDataSetReady(): Promise<boolean> {
    try {
      return await invoke<boolean>('plugin:serialplugin|read_data_set_ready', {
        path: this.options.path
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Reads the RI (Ring Indicator) control signal state
   * @returns {Promise<boolean>} A promise that resolves to the RI state
   */
  async readRingIndicator(): Promise<boolean> {
    try {
      return await invoke<boolean>('plugin:serialplugin|read_ring_indicator', {
        path: this.options.path
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Reads the CD (Carrier Detect) control signal state
   * @returns {Promise<boolean>} A promise that resolves to the CD state
   */
  async readCarrierDetect(): Promise<boolean> {
    try {
      return await invoke<boolean>('plugin:serialplugin|read_carrier_detect', {
        path: this.options.path
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Gets the number of bytes available to read
   * @returns {Promise<number>} A promise that resolves to the number of bytes
   */
  async bytesToRead(): Promise<number> {
    try {
      return await invoke<number>('plugin:serialplugin|bytes_to_read', {
        path: this.options.path
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Gets the number of bytes waiting to be written
   * @returns {Promise<number>} A promise that resolves to the number of bytes
   */
  async bytesToWrite(): Promise<number> {
    try {
      return await invoke<number>('plugin:serialplugin|bytes_to_write', {
        path: this.options.path
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Clears the specified buffer
   * @param {ClearBuffer} buffer The buffer to clear
   * @returns {Promise<void>} A promise that resolves when the buffer is cleared
   */
  async clearBuffer(buffer: ClearBuffer): Promise<void> {
    try {
      return await invoke<void>('plugin:serialplugin|clear_buffer', {
        path: this.options.path,
        bufferType: buffer
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Starts transmitting a break signal
   * @returns {Promise<void>} A promise that resolves when break signal starts
   */
  async setBreak(): Promise<void> {
    try {
      return await invoke<void>('plugin:serialplugin|set_break', {
        path: this.options.path
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Stops transmitting a break signal
   * @returns {Promise<void>} A promise that resolves when break signal stops
   */
  async clearBreak(): Promise<void> {
    try {
      return await invoke<void>('plugin:serialplugin|clear_break', {
        path: this.options.path
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Writes string data to the serial port
   * @param {string} value The data to write
   * @returns {Promise<number>} A promise that resolves to the number of bytes written
   */
  async write(value: string): Promise<number> {
    try {
      if (!this.isOpen) {
        return Promise.reject(`serial port ${this.options.path} not opened!`);
      }

      return await invoke<number>('plugin:serialplugin|write', {
        value,
        path: this.options.path,
      });
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /**
   * @description Writes binary data to the serial port
   * @param {Uint8Array | number[]} value The binary data to write
   * @returns {Promise<number>} A promise that resolves to the number of bytes written
   */
  async writeBinary(value: Uint8Array | number[]): Promise<number> {
    try {
      if (!this.isOpen) {
        return Promise.reject(`serial port ${this.options.path} not opened!`);
      }
      if (value instanceof Uint8Array || value instanceof Array) {
        return await invoke<number>('plugin:serialplugin|write_binary', {
          value: Array.from(value),
          path: this.options.path,
        });
      } else {
        return Promise.reject(
            'value Argument type error! Expected type: string, Uint8Array, number[]',
        );
      }
    } catch (error) {
      return Promise.reject(error);
    }
  }
}

// Export the main class and re-export all types
export {
  SerialPort
};
