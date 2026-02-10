#!/bin/bash
echo "Testing Auth..."

echo "--- Test 1: No Header (Expect 401) ---"
CODE=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:8082/v1/models)
echo "Response: $CODE"
if [ "$CODE" -eq 401 ]; then echo "PASS"; else echo "FAIL"; fi

echo "--- Test 2: Wrong Key (Expect 401) ---"
CODE=$(curl -s -o /dev/null -w "%{http_code}" -H "Authorization: Bearer wrong" http://localhost:8082/v1/models)
echo "Response: $CODE"
if [ "$CODE" -eq 401 ]; then echo "PASS"; else echo "FAIL"; fi

echo "--- Test 3: Correct Key (Expect 200) ---"
CODE=$(curl -s -o /dev/null -w "%{http_code}" -H "Authorization: Bearer secret123" http://localhost:8082/v1/models)
echo "Response: $CODE"
if [ "$CODE" -eq 200 ]; then echo "PASS"; else echo "FAIL"; fi
