import { SearchError } from '@rtepl/gazette/search';
import { AnalyticsError } from '@rtepl/gazette/analytics';
import { WebMcpError } from '@rtepl/gazette/webmcp';

const search = new SearchError('search_data');
const analytics = new AnalyticsError('analytics_environment');
const cause = { operation: 'registration' };
const webmcp = new WebMcpError('webmcp_registration', cause);
const withoutCause = new WebMcpError('webmcp_unsupported');
const searchCode: string = search.code;
const analyticsCode: string = analytics.code;
const webmcpCode: string = webmcp.code;
const retainedCause: unknown = webmcp.cause;
const errors: Error[] = [search, analytics, webmcp, withoutCause];
void [searchCode, analyticsCode, webmcpCode, retainedCause, errors];

// Raw causes are unconstrained values, not ErrorOptions interpreted by this API.
new WebMcpError('webmcp_registration', 'provider unavailable');
new WebMcpError('webmcp_registration', { cause: 'an object retained as supplied' });
new WebMcpError('webmcp_registration', undefined);

// Counterexamples are checked by tsc, never executed.
export function rejectedErrorConstructors() {
  // @ts-expect-error Search errors require their actual string code.
  new SearchError();
  // @ts-expect-error The code cannot be undefined.
  new SearchError(undefined);
  // @ts-expect-error SearchError has no inherited ErrorOptions overload.
  new SearchError('search_data', { cause: 'not supported' });
  // @ts-expect-error Search errors do not coerce non-string codes.
  new SearchError(1);
  // @ts-expect-error Browser tool errors require their actual string code.
  new WebMcpError();
  // @ts-expect-error The code cannot be undefined.
  new WebMcpError(undefined);
  // @ts-expect-error Browser tool errors do not coerce non-string codes.
  new WebMcpError(1);
  // @ts-expect-error The runtime has no third constructor argument.
  new WebMcpError('webmcp_registration', cause, 'extra');
  // @ts-expect-error Analytics errors retain their existing required code.
  new AnalyticsError();
  // @ts-expect-error AnalyticsError has no inherited ErrorOptions overload.
  new AnalyticsError('analytics_environment', { cause: 'not supported' });
  // @ts-expect-error Consumer code cannot replace the declared error code.
  search.code = 'changed';
  // @ts-expect-error Consumer code cannot replace the declared error code.
  webmcp.code = 'changed';
}
