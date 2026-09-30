import { build } from "vite";
import config from "../vite.config.js";

// The Tailwind Vite plugin can leave a native file-system worker alive on
// macOS 27 when invoked through the Vite CLI. Run the build programmatically
// and terminate explicitly once Rollup has finished so Xcode never waits on a
// stale worker.
try {
  await build({ ...config, configFile: false, root: process.cwd() });
} catch (error) {
  console.error(error);
  process.exitCode = 1;
}
