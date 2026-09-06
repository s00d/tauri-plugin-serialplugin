/** @type {import('ts-jest').JestConfigWithTsJest} */
export default {
  preset: 'ts-jest',
  testEnvironment: 'jsdom',
  moduleFileExtensions: ['ts', 'js'],
  transform: {
    '^.+\\.ts$': ['ts-jest', {
      tsconfig: 'tsconfig.json',
    }],
  },
  testMatch: ['**/tests/**/*.test.ts'],
  collectCoverage: true,
  coverageDirectory: 'coverage',
  coverageReporters: ['json', 'lcov', 'text', 'clover'],
  collectCoverageFrom: ['guest-js/**/*.{ts,js}', '!guest-js/**/*.d.ts'],
  coverageThreshold: {
    global: {
      lines: 85,
    },
  },
  coveragePathIgnorePatterns: [
    '/node_modules/',
    '/target/',
    '/examples/',
    '/dist/',
    '/build/',
    '/coverage/',
    '/test-results/'
  ],
  testPathIgnorePatterns: [
    '/node_modules/',
    '/target/',
    '/examples/',
    '/dist/',
    '/build/',
    '/coverage/',
    '/test-results/'
  ],
  reporters: [
    'default',
    ['jest-junit', {
      outputDirectory: 'test-results',
      outputName: 'junit.xml',
      classNameTemplate: '{classname}',
      titleTemplate: '{title}',
      ancestorSeparator: ' › ',
      usePathForSuiteName: true
    }]
  ],
  setupFilesAfterEnv: ['<rootDir>/tests/setup.ts']
};
