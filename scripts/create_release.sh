#!/bin/bash -x

if [ $# -ne 1 ]; then
  echo "usage: $0 <release version>"
  exit 1
fi

RELEASE_VERSION=$1

if [[ $RELEASE_VERSION != v* ]]; then
  echo "RELEASE_VERSION $RELEASE_VERSION does not begin with 'v'"
  exit 1
fi

RELEASE_VERSION_WITHOUT_V=$(echo $RELEASE_VERSION | sed -e 's/^v//g')

echo "RELEASE_VERSION=$RELEASE_VERSION"
echo "RELEASE_VERSION_WITHOUT_V=$RELEASE_VERSION_WITHOUT_V"

cd ~/rust-hyper-unixh2c

toml set Cargo.toml package.version $RELEASE_VERSION_WITHOUT_V > Cargo.toml.tmp
mv Cargo.toml.tmp Cargo.toml

cargo build -v
RESULT=$?
echo "cargo build RESULT = $RESULT"
if [ $RESULT -ne 0 ]; then
  echo "cargo build failed"
fi

cargo test -v
RESULT=$?
echo "cargo test RESULT = $RESULT"
if [ $RESULT -ne 0 ]; then
  echo "cargo test failed"
fi

git add Cargo.toml Cargo.lock || exit 1

git commit -m "Version $RELEASE_VERSION_WITHOUT_V" || exit 1

git tag $RELEASE_VERSION || exit 1

git push -v origin $RELEASE_VERSION || exit 1