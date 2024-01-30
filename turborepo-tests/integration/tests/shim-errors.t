Setup
  $ . ${TESTDIR}/../../helpers/setup.sh

Should error if `--cwd` flag doesn't have path passed along with it
  $ EXPERIMENTAL_RUST_CODEPATH=true ${TURBO} foo bar --cwd
  turbo::shim::empty_cwd
  
    \xc3\x97 No value assigned to `--cwd` flag (esc)
     \xe2\x95\xad\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80 (esc)
   1 \xe2\x94\x82 foo bar --cwd (esc)
     \xc2\xb7         \xe2\x94\x80\xe2\x94\x80\xe2\x94\xac\xe2\x94\x80\xe2\x94\x80 (esc)
     \xc2\xb7           \xe2\x95\xb0\xe2\x94\x80\xe2\x94\x80 Requires a path to be passed after it (esc)
     \xe2\x95\xb0\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80 (esc)
  
  [1]

Should error if multiple `--cwd` flags are passed
  $ EXPERIMENTAL_RUST_CODEPATH=true ${TURBO} --cwd foo --cwd --bar --cwd baz --cwd qux
  turbo::shim::multiple_cwd
  
    \xc3\x97 cannot have multiple `--cwd` flags in command (esc)
     \xe2\x95\xad\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80 (esc)
   1 \xe2\x94\x82 --cwd foo --cwd --bar --cwd baz --cwd qux (esc)
     \xc2\xb7 \xe2\x94\x80\xe2\x94\x80\xe2\x94\xac\xe2\x94\x80\xe2\x94\x80     \xe2\x94\x80\xe2\x94\x80\xe2\x94\xac\xe2\x94\x80\xe2\x94\x80       \xe2\x94\x80\xe2\x94\x80\xe2\x94\xac\xe2\x94\x80\xe2\x94\x80     \xe2\x94\x80\xe2\x94\x80\xe2\x94\xac\xe2\x94\x80\xe2\x94\x80 (esc)
     \xc2\xb7   \xe2\x94\x82         \xe2\x94\x82           \xe2\x94\x82         \xe2\x95\xb0\xe2\x94\x80\xe2\x94\x80 and here (esc)
     \xc2\xb7   \xe2\x94\x82         \xe2\x94\x82           \xe2\x95\xb0\xe2\x94\x80\xe2\x94\x80 and here (esc)
     \xc2\xb7   \xe2\x94\x82         \xe2\x95\xb0\xe2\x94\x80\xe2\x94\x80 but second flag declared here (esc)
     \xc2\xb7   \xe2\x95\xb0\xe2\x94\x80\xe2\x94\x80 first flag declared here (esc)
     \xe2\x95\xb0\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80 (esc)
  
  [1]
