/**
 * URL helpers for joining a baseUrl with an OpenAPI path template and for
 * synthesising stable operation ids when an operation does not declare one.
 */

export function joinBaseAndPath(baseUrl: string, apiPath: string): string {
  if (!baseUrl) return apiPath;
  const base = baseUrl.replace(/\/+$/, "");
  const suffix = apiPath.startsWith("/") ? apiPath : `/${apiPath}`;
  return `${base}${suffix}`;
}

export function sanitizeForOperationId(s: string): string {
  return s
    .replace(/[{}]/g, "")
    .replace(/[^A-Za-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
}

export function syntheticOperationId(method: string, apiPath: string): string {
  const cleaned = sanitizeForOperationId(apiPath);
  return `${method.toLowerCase()}_${cleaned || "root"}`;
}
