#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    const char *root = getenv("APPCOAT_RESOURCE_ROOT");
    if (!root) { fputs("Use appcoat run or provide APPCOAT_RESOURCE_ROOT\n", stderr); return 2; }
    char path[4096];
    int length = snprintf(path, sizeof path, "%s/greeting.txt", root);
    if (length < 0 || (size_t)length >= sizeof path) return 3;
    FILE *resource = fopen(path, "r");
    if (!resource) { perror("resource"); return 4; }
    char buffer[1024];
    while (fgets(buffer, sizeof buffer, resource)) fputs(buffer, stdout);
    int failed = ferror(resource);
    fclose(resource);
    if (failed) return 5;
    for (int i = 1; i < argc; ++i) printf("arg=%s\n", argv[i]);
    return 0;
}
