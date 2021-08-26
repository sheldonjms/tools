#!/bin/bash

echo "Run from directory above samsara project"
echo ""
echo Regenerating Samsara API crate in 10 seconds
echo Press CTRL-C to cancel...
sleep 10

if [ ! -e samsara ];  then
	mkdir samsara
else
    rm -rf samsara/docs samsara/src samsara/Cargo.toml samsara/git_push.sh samsara/README.md
fi

wget -O samsara/samsara-openapi.json https://raw.githubusercontent.com/samsarahq/api-docs/master/swagger.json

java -jar ~/Downloads/openapi-generator-cli.jar generate \
  -i samsara/samsara-openapi.json -g rust -o samsara \
  --library hyper --additional-properties=packageName=samsara
if [ $? -ne 0 ]; then
  echo "ERROR: Cannot generate"
  exit
fi

cd samsara
git add ".openapi-generator/FILES"

# 2021-07-09 Assumes openapi has pull request #9919 integrated to use a more modern version of Hyper.
# https://github.com/OpenAPITools/openapi-generator/pull/9919

