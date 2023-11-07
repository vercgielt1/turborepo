Setup
  $ . ${TESTDIR}/../../../helpers/setup.sh
  $ . ${TESTDIR}/../_helpers/setup_monorepo.sh $(pwd)

Remove comments from our fixture turbo.json so we can do more jq things to it
  $ grep -v '^\s*//' turbo.json > turbo.json.1
  $ mv turbo.json.1 turbo.json

We just created a new file. On Windows, we need to convert it to Unix line endings
so the hashes will be stable with what's expected in our test cases.
  $ if [[ "$OSTYPE" == "msys" ]]; then dos2unix --quiet "$TARGET_DIR/package.json"; fi

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
