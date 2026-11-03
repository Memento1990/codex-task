// Build metadata belongs to this GUI bundle, never the resident Reporter.
import metadata from '../package.json';
export const appVersion: string = metadata.version;
