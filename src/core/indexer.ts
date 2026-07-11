import type { OpenAPIV3 } from "openapi-types";
import { joinBaseAndPath, syntheticOperationId } from "./url";

/**
 * dereferenced 된 OpenAPI 3.x document 를 받아서 endpoint 인덱스를 만든다.
 * `IndexedSpec` 은 endpoints[] + operationId/methodPath 맵 + tag 요약 — 검색 / 응답
 * 빌드의 입력이 된다. `EndpointDetail` 은 한 endpoint 의 풍부한 정보 (parameters,
 * requestBody, responses, examples, fullUrl) 를 담는다.
 */

export type HttpMethod =
  | "GET"
  | "POST"
  | "PUT"
  | "PATCH"
  | "DELETE"
  | "HEAD"
  | "OPTIONS"
  | "TRACE";

export const HTTP_METHODS = [
  "GET",
  "POST",
  "PUT",
  "PATCH",
  "DELETE",
  "HEAD",
  "OPTIONS",
  "TRACE",
] as const satisfies readonly HttpMethod[];

export interface IndexedEndpoint {
  specName: string;
  operationId?: string;
  syntheticOperationId: string;
  method: HttpMethod;
  path: string;
  tags: string[];
  summary?: string;
  description?: string;
  deprecated: boolean;
}

export interface ParameterDetail {
  name: string;
  in: "query" | "header" | "path" | "cookie";
  required: boolean;
  description?: string;
  deprecated: boolean;
  schema?: unknown;
  example?: unknown;
  examples?: Record<string, unknown>;
}

export interface RequestBodyDetail {
  required: boolean;
  description?: string;
  content: Record<string, MediaTypeDetail>;
}

export interface MediaTypeDetail {
  schema?: unknown;
  example?: unknown;
  examples?: Record<string, unknown>;
}

export interface ResponseDetail {
  description: string;
  headers?: Record<string, unknown>;
  content?: Record<string, MediaTypeDetail>;
}

export interface SecurityRequirement {
  [name: string]: string[];
}

export interface ExampleSet {
  request?: Record<string, MediaTypeDetail>;
  responses?: Record<string, Record<string, MediaTypeDetail>>;
}

export interface EndpointDetail extends IndexedEndpoint {
  parameters: ParameterDetail[];
  requestBody?: RequestBodyDetail;
  responses: Record<string, ResponseDetail>;
  security?: SecurityRequirement[];
  examples: ExampleSet;
  fullUrl: string;
  rawOperation: object;
}

export interface IndexedSpec {
  specName: string;
  document: OpenAPIV3.Document;
  endpoints: IndexedEndpoint[];
  byOperationId: Map<string, IndexedEndpoint>;
  byMethodPath: Map<string, IndexedEndpoint>;
  tags: TagSummary[];
}

export interface TagSummary {
  name: string;
  description?: string;
  endpointCount: number;
}

export function indexSpec(
  specName: string,
  document: OpenAPIV3.Document,
): IndexedSpec {
  const endpoints: IndexedEndpoint[] = [];
  const byOperationId = new Map<string, IndexedEndpoint>();
  const byMethodPath = new Map<string, IndexedEndpoint>();
  const tagCounts = new Map<string, number>();

  const paths = document.paths ?? {};
  for (const [pathKey, pathItem] of Object.entries(paths)) {
    if (!pathItem) continue;
    for (const method of HTTP_METHODS) {
      const op = (pathItem as Record<string, unknown>)[method.toLowerCase()] as
        | OpenAPIV3.OperationObject
        | undefined;
      if (!op) continue;

      const synthetic = syntheticOperationId(method, pathKey);
      const tags = Array.isArray(op.tags) ? op.tags.slice() : [];
      const indexed: IndexedEndpoint = {
        specName,
        operationId: op.operationId,
        syntheticOperationId: synthetic,
        method,
        path: pathKey,
        tags,
        summary: op.summary,
        description: op.description,
        deprecated: op.deprecated === true,
      };
      endpoints.push(indexed);

      if (op.operationId) {
        byOperationId.set(op.operationId, indexed);
      }
      byMethodPath.set(methodPathKey(method, pathKey), indexed);

      for (const tag of tags) {
        tagCounts.set(tag, (tagCounts.get(tag) ?? 0) + 1);
      }
    }
  }

  const declaredTags = (document.tags ?? []) as OpenAPIV3.TagObject[];
  const tagDescriptions = new Map<string, string | undefined>();
  for (const t of declaredTags) {
    tagDescriptions.set(t.name, t.description);
    if (!tagCounts.has(t.name)) tagCounts.set(t.name, 0);
  }
  const tags: TagSummary[] = Array.from(tagCounts.entries())
    .map(([name, endpointCount]) => ({
      name,
      description: tagDescriptions.get(name),
      endpointCount,
    }))
    .sort((a, b) => a.name.localeCompare(b.name));

  return { specName, document, endpoints, byOperationId, byMethodPath, tags };
}

export interface ResolveEndpointInput {
  operationId?: string;
  method?: string;
  path?: string;
}

export function resolveEndpoint(
  spec: IndexedSpec,
  input: ResolveEndpointInput,
): IndexedEndpoint | undefined {
  if (input.operationId) {
    const hit = spec.byOperationId.get(input.operationId);
    if (hit) return hit;
    for (const ep of spec.endpoints) {
      if (ep.syntheticOperationId === input.operationId) return ep;
    }
    return undefined;
  }
  if (input.method && input.path) {
    return spec.byMethodPath.get(methodPathKey(input.method, input.path));
  }
  return undefined;
}

export function buildEndpointDetail(
  spec: IndexedSpec,
  endpoint: IndexedEndpoint,
  baseUrl: string,
): EndpointDetail {
  const pathItem = spec.document.paths?.[endpoint.path] as
    | OpenAPIV3.PathItemObject
    | undefined;
  const op = pathItem?.[
    endpoint.method.toLowerCase() as Lowercase<HttpMethod>
  ] as OpenAPIV3.OperationObject | undefined;
  if (!op) {
    throw new Error(
      `internal: operation ${endpoint.method} ${endpoint.path} disappeared from indexed spec`,
    );
  }

  const pathParams = (pathItem?.parameters ??
    []) as OpenAPIV3.ParameterObject[];
  const opParams = (op.parameters ?? []) as OpenAPIV3.ParameterObject[];
  const parameters = mergeParameters(pathParams, opParams).map(
    toParameterDetail,
  );

  const requestBody = op.requestBody
    ? toRequestBodyDetail(op.requestBody as OpenAPIV3.RequestBodyObject)
    : undefined;

  const responses: Record<string, ResponseDetail> = {};
  for (const [status, response] of Object.entries(op.responses ?? {})) {
    responses[status] = toResponseDetail(response as OpenAPIV3.ResponseObject);
  }

  const examples = collectExamples(parameters, requestBody, responses);
  const security = op.security as SecurityRequirement[] | undefined;

  return {
    ...endpoint,
    parameters,
    ...(requestBody !== undefined ? { requestBody } : {}),
    responses,
    ...(security !== undefined ? { security } : {}),
    examples,
    fullUrl: joinBaseAndPath(baseUrl, endpoint.path),
    rawOperation: op,
  };
}

function methodPathKey(method: string, apiPath: string): string {
  return `${method.toUpperCase()} ${apiPath}`;
}

function mergeParameters(
  pathParams: OpenAPIV3.ParameterObject[],
  opParams: OpenAPIV3.ParameterObject[],
): OpenAPIV3.ParameterObject[] {
  const seen = new Map<string, OpenAPIV3.ParameterObject>();
  for (const p of pathParams) seen.set(`${p.in}:${p.name}`, p);
  for (const p of opParams) seen.set(`${p.in}:${p.name}`, p);
  return Array.from(seen.values());
}

function toParameterDetail(p: OpenAPIV3.ParameterObject): ParameterDetail {
  return {
    name: p.name,
    in: p.in as ParameterDetail["in"],
    required: p.required === true || p.in === "path",
    ...(p.description !== undefined ? { description: p.description } : {}),
    deprecated: p.deprecated === true,
    schema: p.schema,
    example: p.example,
    examples: extractExamples(
      p.examples as Record<string, OpenAPIV3.ExampleObject> | undefined,
    ),
  };
}

function toRequestBodyDetail(
  rb: OpenAPIV3.RequestBodyObject,
): RequestBodyDetail {
  const content: Record<string, MediaTypeDetail> = {};
  for (const [mediaType, mt] of Object.entries(rb.content ?? {})) {
    content[mediaType] = toMediaTypeDetail(mt as OpenAPIV3.MediaTypeObject);
  }
  return {
    required: rb.required === true,
    ...(rb.description !== undefined ? { description: rb.description } : {}),
    content,
  };
}

function toResponseDetail(r: OpenAPIV3.ResponseObject): ResponseDetail {
  const content: Record<string, MediaTypeDetail> | undefined = r.content
    ? Object.fromEntries(
        Object.entries(r.content).map(([mt, body]) => [
          mt,
          toMediaTypeDetail(body as OpenAPIV3.MediaTypeObject),
        ]),
      )
    : undefined;
  return {
    description: r.description ?? "",
    headers: r.headers as Record<string, unknown> | undefined,
    ...(content !== undefined ? { content } : {}),
  };
}

function toMediaTypeDetail(mt: OpenAPIV3.MediaTypeObject): MediaTypeDetail {
  return {
    schema: mt.schema,
    example: mt.example,
    examples: extractExamples(
      mt.examples as Record<string, OpenAPIV3.ExampleObject> | undefined,
    ),
  };
}

function extractExamples(
  examples: Record<string, OpenAPIV3.ExampleObject> | undefined,
): Record<string, unknown> | undefined {
  if (!examples) return undefined;
  const out: Record<string, unknown> = {};
  for (const [name, ex] of Object.entries(examples)) {
    out[name] = ex.value;
  }
  return Object.keys(out).length > 0 ? out : undefined;
}

function collectExamples(
  parameters: ParameterDetail[],
  requestBody: RequestBodyDetail | undefined,
  responses: Record<string, ResponseDetail>,
): ExampleSet {
  const set: ExampleSet = {};

  if (requestBody) {
    const reqExamples: Record<string, MediaTypeDetail> = {};
    for (const [mt, body] of Object.entries(requestBody.content)) {
      if (body.example !== undefined || body.examples) {
        reqExamples[mt] = body;
      }
    }
    if (Object.keys(reqExamples).length > 0) set.request = reqExamples;
  }

  const responseExamples: Record<string, Record<string, MediaTypeDetail>> = {};
  for (const [status, response] of Object.entries(responses)) {
    if (!response.content) continue;
    const perStatus: Record<string, MediaTypeDetail> = {};
    for (const [mt, body] of Object.entries(response.content)) {
      if (body.example !== undefined || body.examples) {
        perStatus[mt] = body;
      }
    }
    if (Object.keys(perStatus).length > 0) responseExamples[status] = perStatus;
  }
  if (Object.keys(responseExamples).length > 0)
    set.responses = responseExamples;

  // parameter examples 는 ParameterDetail 안에 그대로 보존된다 — examples 집합엔 따로 안 넣음.
  void parameters;
  return set;
}
