// Centralized logger for the serial plugin
// Avoids circular dependencies and code duplication

export const LogLevel = {
  None: 'None',
  Error: 'Error',
  Warn: 'Warn',
  Info: 'Info',
  Debug: 'Debug',
} as const;

export type LogLevel = (typeof LogLevel)[keyof typeof LogLevel];

// Global log level state
let currentLogLevel: LogLevel = LogLevel.Info;

/**
 * Sets the global log level
 */
export function setLogLevel(level: LogLevel): void {
  currentLogLevel = level;
}

/**
 * Gets the current log level
 */
export function getLogLevel(): LogLevel {
  return currentLogLevel;
}

/**
 * Checks if a message should be logged based on current log level
 */
function shouldLog(level: LogLevel): boolean {
  const levels: LogLevel[] = [
    LogLevel.None,
    LogLevel.Error,
    LogLevel.Warn,
    LogLevel.Info,
    LogLevel.Debug,
  ];
  const currentIndex = levels.indexOf(currentLogLevel);
  const requestedIndex = levels.indexOf(level);
  return requestedIndex <= currentIndex && currentIndex > 0;
}

/**
 * Logs an error message
 */
export function logError(...args: any[]): void {
  if (shouldLog(LogLevel.Error)) {
    console.error(...args);
  }
}

/**
 * Logs a warning message
 */
export function logWarn(...args: any[]): void {
  if (shouldLog(LogLevel.Warn)) {
    console.warn(...args);
  }
}

/**
 * Logs an info message
 */
export function logInfo(...args: any[]): void {
  if (shouldLog(LogLevel.Info)) {
    console.log(...args);
  }
}

/**
 * Logs a debug message
 */
export function logDebug(...args: any[]): void {
  if (shouldLog(LogLevel.Debug)) {
    console.log(...args);
  }
}

