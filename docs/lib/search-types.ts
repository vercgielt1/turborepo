// An edited version of
// https://github.com/CloudCannon/pagefind/blob/production-docs/pagefind_web_js/types/index.d.ts#L128
// Most comments are removed. `Object` as types for properties are also removed.

/** Global index options that can be passed to pagefind.options() */
export interface PagefindIndexOptions {
  basePath?: string;
  baseUrl?: string;
  excerptLength?: number;
  indexWeight?: number;
  mergeFilter?: object;
  highlightParam?: string;
  language?: string;
  primary?: boolean;
}

/** Options that can be passed to pagefind.search() */
export interface PagefindSearchOptions {
  preload?: boolean;
  verbose?: boolean;
  filters?: object;
  sort?: object;
}

/** Filter counts returned from pagefind.filters(), and alongside results from pagefind.search() */
export type PagefindFilterCounts = Record<string, Record<string, number>>;

/** The main results object returned from a call to pagefind.search() */
export interface PagefindSearchResults {
  results: PagefindSearchResult[];
  unfilteredResultCount: number;
  filters: PagefindFilterCounts;
  totalFilters: PagefindFilterCounts;
  timings: {
    preload: number;
    search: number;
    total: number;
  };
}

/** A single result from a search query, before actual data has been loaded */
export interface PagefindSearchResult {
  id: string;
  score: number;
  words: number[];
  data: () => Promise<PagefindSearchFragment>;
}

/** The useful data Pagefind provides for a search result */
export interface PagefindSearchFragment {
  url: string;
  raw_url?: string;
  content: string;
  raw_content?: string;
  excerpt: string;
  sub_results: PagefindSubResult[];
  word_count: number;
  locations: number[];
  weighted_locations: PagefindWordLocation[];
  filters: Record<string, string[]>;
  meta: Record<string, string>;
  anchors: PagefindSearchAnchor[];
}

/** Data for a matched section within a page */
interface PagefindSubResult {
  title: string;
  url: string;
  locations: number[];
  weighted_locations: PagefindWordLocation[];
  excerpt: string;
  anchor?: PagefindSearchAnchor;
}

/** Information about a matching word on a page */
interface PagefindWordLocation {
  weight: number;
  balanced_score: number;
  location: number;
}

/** Raw data about elements with IDs that Pagefind encountered when indexing the page */
interface PagefindSearchAnchor {
  element: string;
  id: string;
  text?: string;
  location: number;
}
