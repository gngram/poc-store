protoc \
  --proto_path=../proto \
  --go_out=./pb \
  --go_opt=paths=source_relative \
  --go_opt=Mtoken.proto=poc-store/jwt-tokens/go/pb \
  ../proto/token.proto

