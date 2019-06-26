#include <stdio.h>
#include <string.h>
#include <stdlib.h> 
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h> 
#include <libgen.h>

int main (int argc, char* argv[])
{
	pid_t pid;
	int status;
	int ret = 0;
        char *process_names[1];
	
	if(argc < 2)
	{
		printf("Usage: ./identity-map <name | pid | command | help> <stable/testing> {arguments}\n");
		printf("Example1: ./identity-map command ls\n");
		printf("Example2: ./identity-map command ./a.out <arguments>\n");
		printf("Example3: ./identity-map name <1/2> blacksholes omp-csr gem5\n");
		printf("Example3: ./identity-map pid 1124 2346 11\n");
		return -1;
	}

	if(strcmp(argv[1],"command")==0)
	{
		if(argc < 3)
		{
			printf("Please provide a command to run\n");
			printf("Example1: ./identity-map command ls\n");
			printf("Example2: ./identity-map command ./a.out <arguments>\n");
			return -1;
		}
	        process_names[0] = basename(argv[2]);
	        ret = syscall(335,process_names,1,1);
	        switch ((pid = fork()))
	        {
	        case -1:
	                perror ("Fork Failed !!");
	                break;
	        case 0:
	                execvp(argv[2], &argv[2]);
	                exit(0);
	                break;
	        default:
	                printf("Badger Trap launched with process %s\n",argv[2]);
	                wait(&status);
	                break;
	        }
	        ret = syscall(335,NULL,0,1);
	}
	else if (strcmp(argv[1],"name")==0 && strcmp(argv[2], "stable")==0)
	{
		ret = syscall(335,&argv[3],argc-3, 1);
	}
	else if (strcmp(argv[1],"name")==0 && strcmp(argv[2], "testing")==0)
	{
		ret = syscall(335,&argv[3],argc-3, 2);
	}
	else if (strcmp(argv[1],"pid")==0)
	{
		ret = syscall(335,&argv[2],argc-2,-1);
	}
	else if((strcmp(argv[1],"help")==0))
	{
		printf("Usage: ./identity-map <names | pid | command | help> {arguments}\n");
                printf("Example1: ./identity-map command ls\n");
                printf("Example2: ./identity-map command ./a.out <arguments>\n");
                printf("Example3: ./identity-map names blacksholes omp-csr gem5\n");
                printf("Example3: ./identity-map pid 1124 2346 11\n");
                return -1;
	}
	else
	{
		printf("Cannot run command provided. Run ./identity-map help for more information\n");
		return -1;
	}

    return ret;
}
