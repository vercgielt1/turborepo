Setup
  $ . ${TESTDIR}/../_helpers/setup_monorepo.sh

 # remove comments from our fixture turbo.json so we can do more jq things to it
  $ grep -v '^\s*//' turbo.json > turbo.json.1
  $ mv turbo.json.1 turbo.json
  $ git commit -am "remove comments" > /dev/null

The fixture does not have a `remoteCache` config at all, output should be null
  $ cat turbo.json | jq .remoteCache
  null

Test that remote caching is enabled by default
  $ ${TURBO} run build --team=vercel --token=hi --output-logs=none | grep "Remote caching"
  \xe2\x80\xa2 Remote caching enabled (esc)

Set `remoteCache = {}` into turbo.json
  $ jq -r --argjson value "{}" '.remoteCache = $value' turbo.json > turbo.json.1
  $ mv turbo.json.1 turbo.json
  $ git commit -am "add empty remote caching config" > /dev/null

Test that remote caching is still enabled
  $ ${TURBO} run build --team=vercel --token=hi --output-logs=none | grep "Remote caching"
  \xe2\x80\xa2 Remote caching enabled (esc)

Set `remoteCache = { enabled: false }` into turbo.json
  $ jq -r --argjson value false '.remoteCache.enabled = $value' turbo.json > turbo.json.1
  $ mv turbo.json.1 turbo.json
  $ git commit -am "disable remote caching" > /dev/null

Test that this time, remote caching is disabled
  $ ${TURBO} run build --team=vercel --token=hi --output-logs=none | grep "Remote caching"
  \xe2\x80\xa2 Remote caching disabled (esc)
