import { Subcommand } from "@aws/amazon-q-developer-cli-shared/internal";

const allCaches: Array<Map<string, unknown>> = [];

export const createCache = <T>() => {
  const cache = new Map<string, T>();
  allCaches.push(cache);
  return cache;
};

export const resetCaches = () => {
  allCaches.forEach((cache) => {
    cache.clear();
  });
};

window.resetCaches = resetCaches;

export const specCache = createCache<Subcommand>();
export const generateSpecCache = createCache<Subcommand>();

window.listCache = () => {
  console.log(specCache);
  console.log(generateSpecCache);
};
