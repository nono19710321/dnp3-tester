#!/bin/bash
# Extract the binary from the docker image
container_id=$(docker create dnp3-linux-arm64-builder)
docker cp $container_id:/tmp/target/release/dnp3_tester ./dnp3_tester_arm64_linux
docker rm -v $container_id
echo "Binary extracted to ./dnp3_tester_arm64_linux"
