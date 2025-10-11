import { logError, logWarn, logInfo } from './logger';

/**
 * Auto-reconnect manager for serial ports
 * Handles automatic reconnection logic with configurable settings
 */
export class AutoReconnectManager {
  private enabled: boolean = false;
  private interval: number = 5000; // 5 seconds default
  private maxAttempts: number | null = 10; // 10 attempts default, null for infinite
  private currentAttempts: number = 0;
  private timer: NodeJS.Timeout | null = null;
  private callback?: (success: boolean, attempt: number) => void;
  private reconnectFunction?: () => Promise<boolean>;

  /**
   * @description Enables auto-reconnect functionality
   * @param {Object} options Auto-reconnect configuration options
   * @param {number} [options.interval=5000] Reconnection interval in milliseconds
   * @param {number | null} [options.maxAttempts=10] Maximum number of reconnection attempts (null for infinite)
   * @param {Function} [options.onReconnect] Callback function called on each reconnection attempt
   * @param {Function} options.reconnectFunction Function that performs the actual reconnection
   * @returns {Promise<void>} A promise that resolves when auto-reconnect is enabled
   */
  async enable(options: {
    interval?: number;
    maxAttempts?: number | null;
    onReconnect?: (success: boolean, attempt: number) => void;
    reconnectFunction: () => Promise<boolean>;
  }): Promise<void> {
    if (this.enabled) {
      logWarn('Auto-reconnect is already enabled');
      return;
    }

    this.enabled = true;
    this.interval = options.interval || 5000;
    this.maxAttempts = options.maxAttempts === undefined ? 10 : options.maxAttempts;
    this.currentAttempts = 0;
    this.callback = options.onReconnect;
    this.reconnectFunction = options.reconnectFunction;

    logInfo(`Auto-reconnect enabled: interval=${this.interval}ms, maxAttempts=${this.maxAttempts === null ? 'infinite' : this.maxAttempts}`);
  }

  /**
   * @description Disables auto-reconnect functionality
   * @returns {Promise<void>} A promise that resolves when auto-reconnect is disabled
   */
  async disable(): Promise<void> {
    if (!this.enabled) {
      logWarn('Auto-reconnect is not enabled');
      return;
    }

    this.enabled = false;
    this.currentAttempts = 0;
    this.callback = undefined;
    this.reconnectFunction = undefined;

    // Clear any pending reconnect timer
    if (this.timer) {
      clearTimeout(this.timer);
      this.timer = null;
    }

    logInfo('Auto-reconnect disabled');
  }

  /**
   * @description Gets auto-reconnect status and configuration
   * @returns {Object} Auto-reconnect information
   */
  getInfo(): {
    enabled: boolean;
    interval: number;
    maxAttempts: number | null;
    currentAttempts: number;
    hasCallback: boolean;
  } {
    return {
      enabled: this.enabled,
      interval: this.interval,
      maxAttempts: this.maxAttempts,
      currentAttempts: this.currentAttempts,
      hasCallback: !!this.callback
    };
  }

  /**
   * @description Starts the auto-reconnect process
   * @returns {Promise<void>} A promise that resolves when auto-reconnect process starts
   */
  async start(): Promise<void> {
    if (!this.enabled || !this.reconnectFunction) {
      return;
    }

    this.currentAttempts = 0;
    await this.performAttempt();
  }

  /**
   * @description Stops the auto-reconnect process (clears timer but keeps enabled)
   * @returns {Promise<void>} A promise that resolves when auto-reconnect process stops
   */
  async stop(): Promise<void> {
    if (this.timer) {
      clearTimeout(this.timer);
      this.timer = null;
    }
  }

  /**
   * @description Resets the attempt counter
   * @returns {Promise<void>} A promise that resolves when counter is reset
   */
  async reset(): Promise<void> {
    this.currentAttempts = 0;
  }

  /**
   * @description Internal method to perform a single reconnection attempt
   * @returns {Promise<void>} A promise that resolves when the attempt is complete
   */
  private async performAttempt(): Promise<void> {
    if (!this.enabled || !this.reconnectFunction) {
      return;
    }

    // Проверяем лимит ДО увеличения счётчика
    if (this.maxAttempts !== null && this.currentAttempts >= this.maxAttempts) {
      logError(`Auto-reconnect failed after ${this.maxAttempts} attempts`);
      if (this.callback) {
        this.callback(false, this.currentAttempts);
      }
      return;
    }
    this.currentAttempts++;

    logInfo(`Auto-reconnect attempt ${this.currentAttempts}${this.maxAttempts !== null ? `/${this.maxAttempts}` : ''}`);

    try {
      const success = await this.reconnectFunction();
      
      if (success) {
        logInfo('Auto-reconnect successful');
        this.currentAttempts = 0; // Reset counter on success
        
        if (this.callback) {
          this.callback(true, this.currentAttempts);
        }
      } else {
        throw new Error('Reconnect function returned false');
      }
    } catch (error) {
      logError(`Auto-reconnect attempt ${this.currentAttempts} failed:`, error);
      
      if (this.callback) {
        this.callback(false, this.currentAttempts);
      }

      // Schedule next attempt
      if (this.enabled) {
        this.timer = setTimeout(() => {
          this.performAttempt();
        }, this.interval);
      }
    }
  }

  isEnabled(): boolean {
    return this.enabled;
  }

  getInterval(): number {
    return this.interval;
  }

  getMaxAttempts(): number | null {
    return this.maxAttempts;
  }

  getCurrentAttempts(): number {
    return this.currentAttempts;
  }

  hasCallback(): boolean {
    return !!this.callback;
  }
} 
