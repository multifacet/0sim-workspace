#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <unistd.h>
#include <sys/types.h>

#define __NR_apriori_paging_alloc 333

void showUsage(void);

int main(int argc,char* argv[])
{
	char proc[32];

	if ( argc < 2 )
	{
		showUsage();
		exit(EXIT_SUCCESS); 
	}
	
	strncpy(proc, argv[1], 31);

	syscall(__NR_apriori_paging_alloc, &argv[1], argc-1, 1);
	puts(proc);

	return 0;
}

void showUsage(void)
{
	printf("\n Usage : ./apriori_paging_set_process [proccess_name]\n");
}
