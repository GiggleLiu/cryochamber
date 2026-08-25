/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** The console's own version, baked in from the crate manifest at build time
   * (see `consoleVersion()` in vite.config.ts). Empty when the build could not
   * read it. */
  readonly VITE_CONSOLE_VERSION: string
}
